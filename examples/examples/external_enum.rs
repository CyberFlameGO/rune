use std::sync::Arc;

use rune::compile::Variant;
use rune::runtime::{Protocol, Vm, VmError};
use rune::termcolor::{ColorChoice, StandardStream};
use rune::{Any, ContextError, Diagnostics, Module, ToValue};

#[derive(Debug, Clone, Copy, Any)]
enum External {
    First(u32),
    Second(u32),
}

fn main() -> rune::Result<()> {
    let m = module()?;

    let mut context = rune_modules::default_context()?;
    context.install(&m)?;
    let runtime = Arc::new(context.runtime());

    let mut sources = rune::sources! {
        entry => {
            pub fn main(external) {
                match external {
                    External::First(value) => value,
                    External::Second(value) => value,
                }
            }
        }
    };

    let mut diagnostics = Diagnostics::new();

    let result = rune::prepare(&mut sources)
        .with_context(&context)
        .with_diagnostics(&mut diagnostics)
        .build();

    if !diagnostics.is_empty() {
        let mut writer = StandardStream::stderr(ColorChoice::Always);
        diagnostics.emit(&mut writer, &sources)?;
    }

    let unit = result?;

    let mut vm = Vm::new(runtime, Arc::new(unit));

    let external = External::First(42);

    let output = vm.call(&["main"], (external,))?;
    // let output = External::from_value(output)?;

    println!("{:?}", output);
    Ok(())
}

/// Construct the `std::generator` module.
pub fn module() -> Result<Module, ContextError> {
    let mut module = Module::new();

    module.ty::<External>()?;

    module
        .enum_meta::<External, 2>([("First", Variant::tuple(1)), ("Second", Variant::tuple(1))])?;

    module.variant_constructor(0, External::First)?;
    module.variant_constructor(1, External::Second)?;

    module.inst_fn(Protocol::IS_VARIANT, |g: &External, index: usize| {
        match (g, index) {
            (External::First(..), 0) => true,
            (External::Second(..), 1) => true,
            _ => false,
        }
    })?;

    module.inst_fn(Protocol::TUPLE_INDEX_GET, |g: &External, index: usize| {
        Ok::<_, VmError>(match (g, index) {
            (External::First(value), 0) => Some(value.to_value()?),
            (External::Second(value), 0) => Some(value.to_value()?),
            _ => None,
        })
    })?;

    Ok(module)
}
