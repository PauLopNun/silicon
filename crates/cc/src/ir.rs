//! The program after its meaning has been worked out: every name resolved,
//! every expression typed, every conversion explicit. The code generator
//! reads only this.

use crate::lex::Loc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Ty {
    U8,
    I8,
    U16,
    I16,
    Ptr(Box<Ty>),
    /// Element type and number of elements.
    Array(Box<Ty>, u32),
    Void,
    /// A constant that has not been given a type yet: `5` is whatever the
    /// expression around it needs it to be.
    Lit,
    /// The type of an expression that was wrong; errors about it have been
    /// reported and no more should follow.
    Error,
}

impl Ty {
    /// Size in bytes.
    pub fn size(&self) -> u32 {
        match self {
            Ty::U8 | Ty::I8 => 1,
            Ty::U16 | Ty::I16 | Ty::Ptr(_) => 2,
            Ty::Array(elem, len) => elem.size() * len,
            Ty::Void | Ty::Lit | Ty::Error => 0,
        }
    }

    /// Is a value of this type 16 bits wide?
    pub fn wide(&self) -> bool {
        self.size() == 2
    }

    pub fn is_int(&self) -> bool {
        matches!(self, Ty::U8 | Ty::I8 | Ty::U16 | Ty::I16)
    }

    pub fn signed(&self) -> bool {
        matches!(self, Ty::I8 | Ty::I16)
    }

    /// Can a variable of this type be read and assigned as a whole?
    pub fn scalar(&self) -> bool {
        self.is_int() || matches!(self, Ty::Ptr(_))
    }

    /// The type as the programmer writes it.
    pub fn name(&self) -> String {
        match self {
            Ty::U8 => "u8".into(),
            Ty::I8 => "i8".into(),
            Ty::U16 => "u16".into(),
            Ty::I16 => "i16".into(),
            Ty::Ptr(to) => format!("{}*", to.name()),
            Ty::Array(elem, len) => format!("{}[{len}]", elem.name()),
            Ty::Void => "void".into(),
            Ty::Lit => "constant".into(),
            Ty::Error => "?".into(),
        }
    }

    /// The value `v` as this type holds it: wrapped to its width and read
    /// with its sign.
    pub fn wrap(&self, v: i64) -> i64 {
        match self {
            Ty::U8 => v as u8 as i64,
            Ty::I8 => v as i8 as i64,
            Ty::U16 | Ty::Ptr(_) => v as u16 as i64,
            Ty::I16 => v as i16 as i64,
            _ => v,
        }
    }
}

/// Where a variable lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Var {
    /// In the function's frame, this many bytes above its bottom.
    Local(u32),
    /// Among the arguments, this many bytes above the return address.
    Param(u32),
    /// At a fixed address: the assembly symbol.
    Global(String),
}

/// Something that can be assigned to.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Place {
    Var(Var),
    /// What a pointer points at.
    Deref(Expr),
}

/// Arithmetic. Where signed and unsigned differ, there are two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    Add,
    Sub,
    Mul,
    DivU,
    DivS,
    ModU,
    ModS,
    And,
    Or,
    Xor,
    Shl,
    /// Shift right bringing in zeroes.
    ShrU,
    /// Shift right keeping the sign.
    ShrS,
}

impl Op {
    pub fn commutes(self) -> bool {
        matches!(self, Op::Add | Op::Mul | Op::And | Op::Or | Op::Xor)
    }
}

/// Comparisons. `>` and `<=` are these with the operands exchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rel {
    Eq,
    Ne,
    LtU,
    GeU,
    LtS,
    GeS,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Expr {
    pub kind: Kind,
    pub ty: Ty,
    pub loc: Loc,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Kind {
    /// A number, already in the range of the expression's type.
    Const(i64),
    /// The address of something with a name in the assembly, plus an offset.
    Sym(String, i64),
    /// The address of a variable of the function.
    Frame(Var),
    Load(Box<Place>),
    /// Assignment; its value is the value stored.
    Store(Box<Place>, Box<Expr>),
    Neg(Box<Expr>),
    /// Every bit inverted.
    Not(Box<Expr>),
    /// Both operands and the result have the same type, except for shifts,
    /// whose count is a `u8`.
    Binary(Op, Box<Expr>, Box<Expr>),
    /// Both operands have the same type; the result is a `u8`, 0 or 1.
    Cmp(Rel, Box<Expr>, Box<Expr>),
    LogNot(Box<Expr>),
    LogAnd(Box<Expr>, Box<Expr>),
    LogOr(Box<Expr>, Box<Expr>),
    /// `c ? a : b`
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    /// 8 bits to 16: with the sign (`true`) or with zeroes.
    Widen(bool, Box<Expr>),
    /// 16 bits to 8: the low byte.
    Narrow(Box<Expr>),
    /// `x++`, `x--`: add to the variable; the value is the old one.
    PostAdd(Box<Place>, i64),
}

impl Expr {
    pub fn constant(&self) -> Option<i64> {
        match self.kind {
            Kind::Const(v) => Some(v),
            _ => None,
        }
    }

    /// Does evaluating this change nothing? (Then it may be evaluated twice,
    /// or not at all.)
    pub fn pure(&self) -> bool {
        match &self.kind {
            Kind::Const(_) | Kind::Sym(..) | Kind::Frame(_) => true,
            Kind::Load(place) => place.pure(),
            Kind::Store(..) | Kind::Call(..) | Kind::PostAdd(..) => false,
            Kind::Neg(a) | Kind::Not(a) | Kind::LogNot(a) | Kind::Widen(_, a) | Kind::Narrow(a) => {
                a.pure()
            }
            Kind::Binary(_, a, b) | Kind::Cmp(_, a, b) | Kind::LogAnd(a, b) | Kind::LogOr(a, b) => {
                a.pure() && b.pure()
            }
            Kind::Cond(c, a, b) => c.pure() && a.pure() && b.pure(),
        }
    }
}

impl Place {
    pub fn pure(&self) -> bool {
        match self {
            Place::Var(_) => true,
            Place::Deref(pointer) => pointer.pure(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Stmt {
    Expr(Expr),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    /// Every loop:
    ///
    /// ```text
    /// top:    if (!before) goto end;       (when there is a `before`)
    ///         body
    /// next:   step                         (`continue` comes here)
    ///         if (after) goto top;         (or always, without an `after`)
    /// end:                                 (`break` comes here)
    /// ```
    Loop {
        before: Option<Expr>,
        body: Vec<Stmt>,
        step: Option<Expr>,
        after: Option<Expr>,
    },
    Return(Option<Expr>, Loc),
    Break(Loc),
    Continue(Loc),
    Asm(String, Loc),
    Block(Vec<Stmt>),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Function {
    pub name: String,
    /// Bytes of local variables.
    pub frame: u32,
    pub body: Vec<Stmt>,
    pub loc: Loc,
    pub end: Loc,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Item {
    Function(Function),
    Asm(String, Loc),
}

/// One value in the initial contents of a variable.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Datum {
    Byte(i64),
    Word(i64),
    /// A 16-bit address: symbol plus offset.
    Addr(String, i64),
}

/// A variable with a fixed address.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Data {
    /// Its assembly symbol.
    pub symbol: String,
    pub size: u32,
    /// In ROM (`const`) or in RAM.
    pub rom: bool,
    /// Initial contents; exactly `size` bytes of them. RAM without any
    /// starts as zeroes.
    pub init: Option<Vec<Datum>>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct Program {
    pub items: Vec<Item>,
    pub data: Vec<Data>,
    /// String literals; number `n` has the symbol `cc_str<n>`.
    pub strings: Vec<Vec<u8>>,
}
