use crate::ast::{Span, Spanned};
use crate::collections::HashMap;
use crate::compile::ir;
use crate::compile::ir::{IrError, IrInterpreter, IrValue};
use crate::query::Used;
use crate::runtime::Shared;
use std::convert::TryFrom;
use std::fmt::Write;

/// Process an ir value as a boolean.
fn as_bool(span: Span, value: IrValue) -> Result<bool, IrError> {
    value
        .into_bool()
        .map_err(|actual| IrError::expected::<_, bool>(span, &actual))
}

/// The outcome of a constant evaluation.
pub enum IrEvalOutcome {
    /// Encountered expression that is not a valid constant expression.
    NotConst(Span),
    /// A compile error.
    Error(IrError),
    /// Break until the next loop, or the optional label.
    Break(Span, IrEvalBreak),
}

impl IrEvalOutcome {
    /// Encountered ast that is not a constant expression.
    pub(crate) fn not_const<S>(spanned: S) -> Self
    where
        S: Spanned,
    {
        Self::NotConst(spanned.span())
    }
}

impl<T> From<T> for IrEvalOutcome
where
    IrError: From<T>,
{
    fn from(error: T) -> Self {
        Self::Error(IrError::from(error))
    }
}

/// The value of a break.
pub enum IrEvalBreak {
    /// Break the next nested loop.
    Inherent,
    /// The break had a value.
    Value(IrValue),
    /// The break had a label.
    Label(Box<str>),
}

fn eval_ir_assign(
    ir: &ir::IrAssign,
    interp: &mut IrInterpreter<'_>,
    used: Used,
) -> Result<IrValue, IrEvalOutcome> {
    interp.budget.take(ir)?;
    let value = eval_ir(&ir.value, interp, used)?;

    interp
        .scopes
        .mut_target(&ir.target, move |t| ir.op.assign(ir, t, value))?;

    Ok(IrValue::Unit)
}

fn eval_ir_binary(
    ir: &ir::IrBinary,
    interp: &mut IrInterpreter<'_>,
    used: Used,
) -> Result<IrValue, IrEvalOutcome> {
    use std::ops::{Add, Mul, Shl, Shr, Sub};

    let span = ir.span();
    interp.budget.take(span)?;

    let a = eval_ir(&ir.lhs, interp, used)?;
    let b = eval_ir(&ir.rhs, interp, used)?;

    match (a, b) {
        (IrValue::Integer(a), IrValue::Integer(b)) => match ir.op {
            ir::IrBinaryOp::Add => {
                return Ok(IrValue::Integer(a.add(&b)));
            }
            ir::IrBinaryOp::Sub => {
                return Ok(IrValue::Integer(a.sub(&b)));
            }
            ir::IrBinaryOp::Mul => {
                return Ok(IrValue::Integer(a.mul(&b)));
            }
            ir::IrBinaryOp::Div => {
                let number = a
                    .checked_div(&b)
                    .ok_or_else(|| IrError::msg(span, "division by zero"))?;
                return Ok(IrValue::Integer(number));
            }
            ir::IrBinaryOp::Shl => {
                let b = u32::try_from(b)
                    .map_err(|_| IrError::msg(&ir.rhs, "cannot be converted to shift operand"))?;

                let n = a.shl(b);
                return Ok(IrValue::Integer(n));
            }
            ir::IrBinaryOp::Shr => {
                let b = u32::try_from(b)
                    .map_err(|_| IrError::msg(&ir.rhs, "cannot be converted to shift operand"))?;

                let n = a.shr(b);
                return Ok(IrValue::Integer(n));
            }
            ir::IrBinaryOp::Lt => return Ok(IrValue::Bool(a < b)),
            ir::IrBinaryOp::Lte => return Ok(IrValue::Bool(a <= b)),
            ir::IrBinaryOp::Eq => return Ok(IrValue::Bool(a == b)),
            ir::IrBinaryOp::Gt => return Ok(IrValue::Bool(a > b)),
            ir::IrBinaryOp::Gte => return Ok(IrValue::Bool(a >= b)),
        },
        (IrValue::Float(a), IrValue::Float(b)) => {
            #[allow(clippy::float_cmp)]
            match ir.op {
                ir::IrBinaryOp::Add => return Ok(IrValue::Float(a + b)),
                ir::IrBinaryOp::Sub => return Ok(IrValue::Float(a - b)),
                ir::IrBinaryOp::Mul => return Ok(IrValue::Float(a * b)),
                ir::IrBinaryOp::Div => return Ok(IrValue::Float(a / b)),
                ir::IrBinaryOp::Lt => return Ok(IrValue::Bool(a < b)),
                ir::IrBinaryOp::Lte => return Ok(IrValue::Bool(a <= b)),
                ir::IrBinaryOp::Eq => return Ok(IrValue::Bool(a == b)),
                ir::IrBinaryOp::Gt => return Ok(IrValue::Bool(a > b)),
                ir::IrBinaryOp::Gte => return Ok(IrValue::Bool(a >= b)),
                _ => (),
            };
        }
        (IrValue::String(a), IrValue::String(b)) => {
            if let ir::IrBinaryOp::Add = ir.op {
                return Ok(IrValue::String(add_strings(span, &a, &b)?));
            }
        }
        _ => (),
    }

    return Err(IrEvalOutcome::not_const(span));

    fn add_strings(
        span: Span,
        a: &Shared<String>,
        b: &Shared<String>,
    ) -> Result<Shared<String>, IrError> {
        let a = a.borrow_ref().map_err(|e| IrError::new(span, e))?;
        let b = b.borrow_ref().map_err(|e| IrError::new(span, e))?;

        let mut a = String::from(&*a);
        a.push_str(&b);
        Ok(Shared::new(a))
    }
}

fn eval_ir_branches(
    ir: &ir::IrBranches,
    interp: &mut IrInterpreter<'_>,
    used: Used,
) -> Result<IrValue, IrEvalOutcome> {
    for (ir_condition, branch) in &ir.branches {
        let guard = interp.scopes.push();

        let value = eval_ir_condition(ir_condition, interp, used)?;

        let output = if as_bool(ir_condition.span(), value)? {
            Some(eval_ir_scope(branch, interp, used)?)
        } else {
            None
        };

        interp.scopes.pop(branch, guard)?;

        if let Some(output) = output {
            return Ok(output);
        }
    }

    if let Some(branch) = &ir.default_branch {
        return eval_ir_scope(branch, interp, used);
    }

    Ok(IrValue::Unit)
}

fn eval_ir_call(
    ir: &ir::IrCall,
    interp: &mut IrInterpreter<'_>,
    used: Used,
) -> Result<IrValue, IrEvalOutcome> {
    let mut args = Vec::new();

    for arg in &ir.args {
        args.push(eval_ir(arg, interp, used)?);
    }

    Ok(interp.call_const_fn(ir, &ir.target, args, used)?)
}

fn eval_ir_condition(
    ir: &ir::IrCondition,
    interp: &mut IrInterpreter<'_>,
    used: Used,
) -> Result<IrValue, IrEvalOutcome> {
    Ok(IrValue::Bool(match ir {
        ir::IrCondition::Ir(ir) => {
            let value = eval_ir(ir, interp, used)?;
            as_bool(ir.span(), value)?
        }
        ir::IrCondition::Let(ir_let) => {
            let value = eval_ir(&ir_let.ir, interp, used)?;
            ir_let.pat.matches(interp, value, ir)?
        }
    }))
}

fn eval_ir_decl(
    ir: &ir::IrDecl,
    interp: &mut IrInterpreter<'_>,
    used: Used,
) -> Result<IrValue, IrEvalOutcome> {
    interp.budget.take(ir)?;
    let value = eval_ir(&ir.value, interp, used)?;
    interp.scopes.decl(&ir.name, value, ir)?;
    Ok(IrValue::Unit)
}

fn eval_ir_loop(
    ir: &ir::IrLoop,
    interp: &mut IrInterpreter<'_>,
    used: Used,
) -> Result<IrValue, IrEvalOutcome> {
    let span = ir.span();
    interp.budget.take(span)?;

    let guard = interp.scopes.push();

    loop {
        if let Some(condition) = &ir.condition {
            interp.scopes.clear_current(&*condition)?;

            let value = eval_ir_condition(&*condition, interp, used)?;

            if !as_bool(condition.span(), value)? {
                break;
            }
        }

        match eval_ir_scope(&ir.body, interp, used) {
            Ok(..) => (),
            Err(outcome) => match outcome {
                IrEvalOutcome::Break(span, b) => match b {
                    IrEvalBreak::Inherent => break,
                    IrEvalBreak::Label(l) => {
                        if ir.label.as_ref() == Some(&l) {
                            break;
                        }

                        return Err(IrEvalOutcome::Break(span, IrEvalBreak::Label(l)));
                    }
                    IrEvalBreak::Value(value) => {
                        if ir.condition.is_none() {
                            return Ok(value);
                        }

                        return Err(IrEvalOutcome::from(IrError::msg(
                            span,
                            "break with value is not supported for unconditional loops",
                        )));
                    }
                },
                outcome => return Err(outcome),
            },
        };
    }

    interp.scopes.pop(ir, guard)?;
    Ok(IrValue::Unit)
}

fn eval_ir_object(
    ir: &ir::IrObject,
    interp: &mut IrInterpreter<'_>,
    used: Used,
) -> Result<IrValue, IrEvalOutcome> {
    let mut object = HashMap::with_capacity(ir.assignments.len());

    for (key, value) in ir.assignments.iter() {
        object.insert(key.as_ref().to_owned(), eval_ir(value, interp, used)?);
    }

    Ok(IrValue::Object(Shared::new(object)))
}

fn eval_ir_scope(
    ir: &ir::IrScope,
    interp: &mut IrInterpreter<'_>,
    used: Used,
) -> Result<IrValue, IrEvalOutcome> {
    interp.budget.take(ir)?;
    let guard = interp.scopes.push();

    for ir in &ir.instructions {
        let _ = eval_ir(ir, interp, used)?;
    }

    let value = if let Some(last) = &ir.last {
        eval_ir(last, interp, used)?
    } else {
        IrValue::Unit
    };

    interp.scopes.pop(ir, guard)?;
    Ok(value)
}

fn eval_ir_set(
    ir: &ir::IrSet,
    interp: &mut IrInterpreter<'_>,
    used: Used,
) -> Result<IrValue, IrEvalOutcome> {
    interp.budget.take(ir)?;
    let value = eval_ir(&ir.value, interp, used)?;
    interp.scopes.set_target(&ir.target, value)?;
    Ok(IrValue::Unit)
}

fn eval_ir_template(
    ir: &ir::IrTemplate,
    interp: &mut IrInterpreter<'_>,
    used: Used,
) -> Result<IrValue, IrEvalOutcome> {
    interp.budget.take(ir)?;

    let mut buf = String::new();

    for component in &ir.components {
        match component {
            ir::IrTemplateComponent::String(string) => {
                buf.push_str(string);
            }
            ir::IrTemplateComponent::Ir(ir) => {
                let const_value = eval_ir(ir, interp, used)?;

                match const_value {
                    IrValue::Integer(integer) => {
                        write!(buf, "{}", integer).unwrap();
                    }
                    IrValue::Float(float) => {
                        let mut buffer = ryu::Buffer::new();
                        buf.push_str(buffer.format(float));
                    }
                    IrValue::Bool(b) => {
                        write!(buf, "{}", b).unwrap();
                    }
                    IrValue::String(s) => {
                        let s = s.borrow_ref().map_err(IrError::access(ir))?;
                        buf.push_str(&*s);
                    }
                    _ => {
                        return Err(IrEvalOutcome::not_const(ir));
                    }
                }
            }
        }
    }

    Ok(IrValue::String(Shared::new(buf)))
}

fn eval_ir_tuple(
    ir: &ir::IrTuple,
    interp: &mut IrInterpreter<'_>,
    used: Used,
) -> Result<IrValue, IrEvalOutcome> {
    let mut items = Vec::with_capacity(ir.items.len());

    for item in ir.items.iter() {
        items.push(eval_ir(item, interp, used)?);
    }

    Ok(IrValue::Tuple(Shared::new(items.into_boxed_slice())))
}

fn eval_ir_vec(
    ir: &ir::IrVec,
    interp: &mut IrInterpreter<'_>,
    used: Used,
) -> Result<IrValue, IrEvalOutcome> {
    let mut vec = Vec::with_capacity(ir.items.len());

    for item in ir.items.iter() {
        vec.push(eval_ir(item, interp, used)?);
    }

    Ok(IrValue::Vec(Shared::new(vec)))
}

/// IrEval the interior expression.
pub(crate) fn eval_ir(
    ir: &ir::Ir,
    interp: &mut IrInterpreter<'_>,
    used: Used,
) -> Result<IrValue, IrEvalOutcome> {
    interp.budget.take(ir)?;

    match &ir.kind {
        ir::IrKind::Scope(ir) => eval_ir_scope(ir, interp, used),
        ir::IrKind::Binary(ir) => eval_ir_binary(ir, interp, used),
        ir::IrKind::Decl(ir) => eval_ir_decl(ir, interp, used),
        ir::IrKind::Set(ir) => eval_ir_set(ir, interp, used),
        ir::IrKind::Assign(ir) => eval_ir_assign(ir, interp, used),
        ir::IrKind::Template(ir) => eval_ir_template(ir, interp, used),
        ir::IrKind::Name(name) => Ok(interp.resolve_var(ir.span(), name.as_ref(), used)?),
        ir::IrKind::Target(target) => Ok(interp.scopes.get_target(target)?),
        ir::IrKind::Value(value) => Ok(value.clone()),
        ir::IrKind::Branches(ir) => eval_ir_branches(ir, interp, used),
        ir::IrKind::Loop(ir) => eval_ir_loop(ir, interp, used),
        ir::IrKind::Break(ir) => Err(ir.as_outcome(interp, used)),
        ir::IrKind::Vec(ir) => eval_ir_vec(ir, interp, used),
        ir::IrKind::Tuple(ir) => eval_ir_tuple(ir, interp, used),
        ir::IrKind::Object(ir) => eval_ir_object(ir, interp, used),
        ir::IrKind::Call(ir) => eval_ir_call(ir, interp, used),
    }
}
