use crate::ast;
use crate::{Parse, ParseError, Parser, Spanned};
use runestick::Span;

/// A block of expressions.
#[derive(Debug, Clone)]
pub struct ExprBlock {
    /// The attributes for the block.
    pub attributes: Vec<ast::Attribute>,
    /// The close brace.
    pub block: ast::Block,
}

into_tokens!(ExprBlock { attributes, block });

impl ExprBlock {
    /// Test if the block expression doesn't produce a value.
    pub fn produces_nothing(&self) -> bool {
        self.block.produces_nothing()
    }

    /// Test if the block is a constant expression.
    pub fn is_const(&self) -> bool {
        self.block.is_const()
    }

    /// Parse the block expression attaching the given attributes
    pub fn parse_with_attributes(
        parser: &mut Parser<'_>,
        attributes: Vec<ast::Attribute>,
    ) -> Result<Self, ParseError> {
        Ok(Self {
            attributes,
            block: parser.parse()?,
        })
    }
}

impl Spanned for ExprBlock {
    fn span(&self) -> Span {
        if let Some(first) = self.attributes.first() {
            first.span().join(self.block.span())
        } else {
            self.block.span()
        }
    }
}

impl Parse for ExprBlock {
    fn parse(parser: &mut Parser<'_>) -> Result<Self, ParseError> {
        let attributes = parser.parse()?;
        Self::parse_with_attributes(parser, attributes)
    }
}
