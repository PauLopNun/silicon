//! The program as it was written: a tree, with no meaning attached yet.

use crate::lex::Loc;

/// The four integer types, and `void`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Base {
    U8,
    I8,
    U16,
    I16,
    Void,
}

/// A type as written: a base type and how many `*` follow it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TypeName {
    pub base: Base,
    pub pointers: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnOp {
    Neg,
    BitNot,
    Not,
    Deref,
    Addr,
    PreInc,
    PreDec,
    PostInc,
    PostDec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    LogAnd,
    LogOr,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Expr {
    pub kind: ExprKind,
    pub loc: Loc,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ExprKind {
    Num(i64),
    Str(Vec<u8>),
    Ident(String),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// `a = b`, or `a op= b`.
    Assign(Option<BinOp>, Box<Expr>, Box<Expr>),
    /// `c ? a : b`
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    Index(Box<Expr>, Box<Expr>),
    Cast(TypeName, Box<Expr>),
    SizeofType(TypeName),
    SizeofExpr(Box<Expr>),
}

/// The initial value of a variable.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Init {
    Expr(Expr),
    /// `{ a, b, c }`
    List(Vec<Expr>, Loc),
}

/// One declared variable: `u8 *name[size] = init`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Decl {
    pub is_const: bool,
    pub ty: TypeName,
    pub name: String,
    /// `Some(None)` is `name[]`: the size comes from the initial value.
    pub array: Option<Option<Expr>>,
    pub init: Option<Init>,
    pub loc: Loc,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Stmt {
    Block(Vec<Stmt>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    DoWhile(Box<Stmt>, Expr),
    /// `for (init; condition; step) body`
    For(Option<Box<Stmt>>, Option<Expr>, Option<Expr>, Box<Stmt>),
    Return(Option<Expr>, Loc),
    Break(Loc),
    Continue(Loc),
    /// One or more variables declared in one statement.
    Locals(Vec<Decl>),
    Expr(Expr),
    Asm(String, Loc),
    Empty,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Param {
    pub ty: TypeName,
    pub name: String,
    pub loc: Loc,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Function {
    pub ret: TypeName,
    pub name: String,
    pub params: Vec<Param>,
    /// `None` for a prototype: the function is defined elsewhere, perhaps
    /// in assembly.
    pub body: Option<Vec<Stmt>>,
    pub loc: Loc,
    /// Where the body ends, for the code that returns.
    pub end: Loc,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Item {
    Asm(String, Loc),
    Global(Decl),
    Function(Function),
}
