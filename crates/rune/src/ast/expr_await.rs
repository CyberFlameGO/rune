use crate::ast::prelude::*;

/// A return statement `<expr>.await`.
///
/// # Examples
///
/// ```
/// use rune::{ast, testing};
///
/// testing::roundtrip::<ast::Expr>("(42).await");
/// testing::roundtrip::<ast::Expr>("self.await");
/// testing::roundtrip::<ast::Expr>("test.await");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, ToTokens, Spanned)]
#[non_exhaustive]
pub struct ExprAwait {
    /// Attributes associated with expression.
    #[rune(iter)]
    pub attributes: Vec<ast::Attribute>,
    /// The expression being awaited.
    pub expr: Box<ast::Expr>,
    /// The dot separating the expression.
    pub dot: T![.],
    /// The await token.
    pub await_token: T![await],
}

expr_parse!(Await, ExprAwait, ".await expression");
