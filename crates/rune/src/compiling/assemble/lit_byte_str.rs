use crate::compiling::assemble::prelude::*;

/// Compile a literal string `b"Hello World"`.
impl Assemble for ast::LitByteStr {
    fn assemble(&self, c: &mut Compiler<'_>, needs: Needs) -> CompileResult<Value> {
        let span = self.span();
        log::trace!("LitByteStr => {:?}", c.source.source(span));

        // NB: Elide the entire literal if it's not needed.
        if !needs.value() {
            c.warnings.not_used(c.source_id, span, c.context());
            return Ok(Value::empty(span));
        }

        let bytes = self.resolve(&c.storage, &*c.source)?;
        let slot = c.unit.new_static_bytes(span, &*bytes)?;
        c.asm.push(Inst::Bytes { slot }, span);
        Ok(Value::unnamed(span, c))
    }
}
