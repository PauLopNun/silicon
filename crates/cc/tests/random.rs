//! Random programs against a reference.
//!
//! The reference is a few lines of plain Rust that say what an expression
//! of the language is worth (`Expr::eval`). The compiler is given the same
//! expressions as text, and what the compiled program prints must be what
//! the reference computed. This is the compiler's version of the rule that
//! every piece of the machine has a plain function it is compared with.

mod common;

use proptest::prelude::*;

/// The four integer types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ty {
    U8,
    I8,
    U16,
    I16,
}

use Ty::{I16, I8, U16, U8};

impl Ty {
    fn name(self) -> &'static str {
        match self {
            U8 => "u8",
            I8 => "i8",
            U16 => "u16",
            I16 => "i16",
        }
    }

    fn bits(self) -> i64 {
        if matches!(self, U8 | I8) {
            8
        } else {
            16
        }
    }

    fn signed(self) -> bool {
        matches!(self, I8 | I16)
    }

    /// The number a value of this type is, given any integer with the
    /// right low bits.
    fn wrap(self, v: i64) -> i64 {
        match self {
            U8 => v as u8 as i64,
            I8 => v as i8 as i64,
            U16 => v as u16 as i64,
            I16 => v as i16 as i64,
        }
    }

    /// The type two operands are brought to (`docs/LANGUAGE.md`): the wider
    /// one, and unsigned if they differ only in sign.
    fn with(self, other: Ty) -> Ty {
        match (self.bits(), other.bits()) {
            _ if self == other => self,
            (8, 8) => U8,
            (16, 16) => U16,
            (16, _) => self,
            _ => other,
        }
    }
}

/// The variables of every generated program: four globals, four local
/// variables and four parameters, one of each type.
const VARS: [(&str, Ty); 12] = [
    ("g0", U8),
    ("g1", I8),
    ("g2", U16),
    ("g3", I16),
    ("l0", U8),
    ("l1", I8),
    ("l2", U16),
    ("l3", I16),
    ("p0", U8),
    ("p1", I8),
    ("p2", U16),
    ("p3", I16),
];

/// A table in ROM.
const ROM8: [u8; 16] = [
    3, 141, 59, 26, 53, 58, 97, 93, 238, 46, 26, 43, 0, 255, 128, 127,
];

/// Everything a program can change.
#[derive(Clone, Debug)]
struct State {
    vars: [i64; 12],
    ram16: [u16; 8],
    out: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    And,
    Or,
    Xor,
}

#[derive(Clone, Copy, Debug)]
enum Rel {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Debug)]
enum Expr {
    Const(Ty, i64),
    Var(usize),
    Cast(Ty, Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
    /// Left (`true`) or right, by a computed count.
    Shift(bool, Box<Expr>, Box<Expr>),
    /// ... by a count written as a number.
    ShiftBy(bool, Box<Expr>, u8),
    Cmp(Rel, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
    Inv(Box<Expr>),
    /// `rom8[i & 15]`, and the same through a pointer.
    Rom8(bool, Box<Expr>),
    /// `ram16[i & 7]`, and the same through a pointer.
    Ram16(bool, Box<Expr>),
    /// Calls of the three functions every program has.
    Id8(Box<Expr>),
    Sum3(Box<Expr>, Box<Expr>, Box<Expr>),
    Pick(Box<Expr>, Box<Expr>, Box<Expr>),
}

impl Expr {
    fn ty(&self) -> Ty {
        match self {
            Expr::Const(ty, _) | Expr::Cast(ty, _) => *ty,
            Expr::Var(i) => VARS[*i].1,
            Expr::Bin(_, x, y) | Expr::Cond(_, x, y) => x.ty().with(y.ty()),
            Expr::Shift(_, x, _) | Expr::ShiftBy(_, x, _) | Expr::Neg(x) | Expr::Inv(x) => x.ty(),
            Expr::Cmp(..) | Expr::And(..) | Expr::Or(..) | Expr::Not(_) => U8,
            Expr::Rom8(..) | Expr::Id8(_) => U8,
            Expr::Ram16(..) | Expr::Sum3(..) => U16,
            Expr::Pick(..) => I16,
        }
    }

    /// The expression as the programmer writes it.
    fn text(&self) -> String {
        match self {
            Expr::Const(ty, v) => format!("({}){v}", ty.name()),
            Expr::Var(i) => VARS[*i].0.to_string(),
            Expr::Cast(ty, x) => format!("({})({})", ty.name(), x.text()),
            Expr::Bin(op, x, y) => {
                let (x, y) = (x.text(), y.text());
                match op {
                    Op::Add => format!("({x} + {y})"),
                    Op::Sub => format!("({x} - {y})"),
                    Op::Mul => format!("({x} * {y})"),
                    // (never by zero: that is tested by hand)
                    Op::Div => format!("({x} / ({y} | 1))"),
                    Op::Mod => format!("({x} % ({y} | 1))"),
                    Op::And => format!("({x} & {y})"),
                    Op::Or => format!("({x} | {y})"),
                    Op::Xor => format!("({x} ^ {y})"),
                }
            }
            Expr::Shift(true, x, n) => format!("({} << {})", x.text(), n.text()),
            Expr::Shift(false, x, n) => format!("({} >> {})", x.text(), n.text()),
            Expr::ShiftBy(true, x, n) => format!("({} << {n})", x.text()),
            Expr::ShiftBy(false, x, n) => format!("({} >> {n})", x.text()),
            Expr::Cmp(rel, x, y) => {
                let sign = match rel {
                    Rel::Eq => "==",
                    Rel::Ne => "!=",
                    Rel::Lt => "<",
                    Rel::Le => "<=",
                    Rel::Gt => ">",
                    Rel::Ge => ">=",
                };
                format!("({} {sign} {})", x.text(), y.text())
            }
            Expr::And(x, y) => format!("({} && {})", x.text(), y.text()),
            Expr::Or(x, y) => format!("({} || {})", x.text(), y.text()),
            Expr::Not(x) => format!("(!{})", x.text()),
            Expr::Cond(c, x, y) => format!("({} ? {} : {})", c.text(), x.text(), y.text()),
            Expr::Neg(x) => format!("(-{})", x.text()),
            Expr::Inv(x) => format!("(~{})", x.text()),
            Expr::Rom8(false, i) => format!("rom8[{} & 15]", i.text()),
            Expr::Rom8(true, i) => format!("pb[{} & 15]", i.text()),
            Expr::Ram16(false, i) => format!("ram16[{} & 7]", i.text()),
            Expr::Ram16(true, i) => format!("pw[{} & 7]", i.text()),
            Expr::Id8(x) => format!("id8({})", x.text()),
            Expr::Sum3(x, y, z) => format!("sum3({}, {}, {})", x.text(), y.text(), z.text()),
            Expr::Pick(c, x, y) => format!("pick({}, {}, {})", c.text(), x.text(), y.text()),
        }
    }

    /// What the expression is worth: the reference.
    fn eval(&self, s: &State) -> i64 {
        let ty = self.ty();
        match self {
            Expr::Const(_, v) => *v,
            Expr::Var(i) => s.vars[*i],
            Expr::Cast(to, x) => to.wrap(x.eval(s)),
            Expr::Bin(op, x, y) => {
                let (x, y) = (ty.wrap(x.eval(s)), y.eval(s));
                let y = match op {
                    Op::Div | Op::Mod => ty.wrap(y | 1),
                    _ => ty.wrap(y),
                };
                ty.wrap(match op {
                    Op::Add => x + y,
                    Op::Sub => x - y,
                    Op::Mul => x * y,
                    // Towards zero, the remainder with the sign of x.
                    Op::Div => x / y,
                    Op::Mod => x % y,
                    Op::And => x & y,
                    Op::Or => x | y,
                    Op::Xor => x ^ y,
                })
            }
            Expr::Shift(left, x, n) => shift(ty, *left, x.eval(s), n.eval(s) & 0xff),
            Expr::ShiftBy(left, x, n) => shift(ty, *left, x.eval(s), *n as i64),
            Expr::Cmp(rel, x, y) => {
                let both = x.ty().with(y.ty());
                let (x, y) = (both.wrap(x.eval(s)), both.wrap(y.eval(s)));
                let holds = match rel {
                    Rel::Eq => x == y,
                    Rel::Ne => x != y,
                    Rel::Lt => x < y,
                    Rel::Le => x <= y,
                    Rel::Gt => x > y,
                    Rel::Ge => x >= y,
                };
                holds as i64
            }
            Expr::And(x, y) => (x.eval(s) != 0 && y.eval(s) != 0) as i64,
            Expr::Or(x, y) => (x.eval(s) != 0 || y.eval(s) != 0) as i64,
            Expr::Not(x) => (x.eval(s) == 0) as i64,
            Expr::Cond(c, x, y) => ty.wrap(if c.eval(s) != 0 { x.eval(s) } else { y.eval(s) }),
            Expr::Neg(x) => ty.wrap(-x.eval(s)),
            Expr::Inv(x) => ty.wrap(!x.eval(s)),
            Expr::Rom8(_, i) => ROM8[(i.eval(s) & 15) as usize] as i64,
            Expr::Ram16(_, i) => s.ram16[(i.eval(s) & 7) as usize] as i64,
            Expr::Id8(x) => U8.wrap(x.eval(s)),
            Expr::Sum3(x, y, z) => {
                U16.wrap(U16.wrap(x.eval(s)) + U8.wrap(y.eval(s)) + I16.wrap(z.eval(s)))
            }
            Expr::Pick(c, x, y) => I16.wrap(if U8.wrap(c.eval(s)) != 0 {
                x.eval(s)
            } else {
                y.eval(s)
            }),
        }
    }
}

/// `x << n` or `x >> n` in type `ty`. A count as large as the type, or
/// larger, shifts everything out.
fn shift(ty: Ty, left: bool, x: i64, n: i64) -> i64 {
    match (left, ty.signed()) {
        (true, _) if n >= ty.bits() => 0,
        (true, _) => ty.wrap(x << n),
        (false, false) if n >= ty.bits() => 0,
        // What comes in at the top is the sign, or zeroes.
        (false, _) => x >> n.min(63),
    }
}

#[derive(Clone, Debug)]
enum Stmt {
    /// `v = e;`
    Assign(usize, Expr),
    /// `v op= e;`
    OpAssign(usize, Op, Expr),
    /// `v <<= e;`, `v >>= e;`
    ShiftAssign(usize, bool, Expr),
    /// `v++;`, `v--;`
    Step(usize, bool),
    /// `show(v++);` and the other three.
    ShowStep(usize, bool, bool),
    /// `ram16[i & 7] = e;`, or through the pointer.
    Store(bool, Expr, Expr),
    /// `show(e);`
    Show(Expr),
    If(Expr, Box<Stmt>, Box<Stmt>),
    /// `for (u8 k = 0; k < n; k++) s`
    Repeat(u8, Box<Stmt>),
}

impl Stmt {
    fn text(&self) -> String {
        let name = |i: &usize| VARS[*i].0;
        match self {
            Stmt::Assign(i, e) => format!("{} = {};", name(i), e.text()),
            Stmt::OpAssign(i, op, e) => {
                let e = e.text();
                let sign = match op {
                    Op::Add => "+=",
                    Op::Sub => "-=",
                    Op::Mul => "*=",
                    Op::Div => return format!("{} /= ({e} | 1);", name(i)),
                    Op::Mod => return format!("{} %= ({e} | 1);", name(i)),
                    Op::And => "&=",
                    Op::Or => "|=",
                    Op::Xor => "^=",
                };
                format!("{} {sign} {e};", name(i))
            }
            Stmt::ShiftAssign(i, true, e) => format!("{} <<= {};", name(i), e.text()),
            Stmt::ShiftAssign(i, false, e) => format!("{} >>= {};", name(i), e.text()),
            Stmt::Step(i, true) => format!("{}++;", name(i)),
            Stmt::Step(i, false) => format!("{}--;", name(i)),
            Stmt::ShowStep(i, up, post) => {
                let sign = if *up { "++" } else { "--" };
                if *post {
                    format!("show({}{sign});", name(i))
                } else {
                    format!("show({sign}{});", name(i))
                }
            }
            Stmt::Store(false, i, e) => format!("ram16[{} & 7] = {};", i.text(), e.text()),
            Stmt::Store(true, i, e) => format!("pw[{} & 7] = {};", i.text(), e.text()),
            Stmt::Show(e) => format!("show({});", e.text()),
            Stmt::If(c, then, otherwise) => {
                format!(
                    "if ({}) {{ {} }} else {{ {} }}",
                    c.text(),
                    then.text(),
                    otherwise.text()
                )
            }
            Stmt::Repeat(n, body) => format!("for (u8 k = 0; k < {n}; k++) {{ {} }}", body.text()),
        }
    }

    /// Do what the statement says: the reference.
    fn run(&self, s: &mut State) {
        use std::fmt::Write;
        match self {
            Stmt::Assign(i, e) => s.vars[*i] = VARS[*i].1.wrap(e.eval(s)),
            Stmt::OpAssign(i, op, e) => {
                let value = Expr::Bin(*op, Box::new(Expr::Var(*i)), Box::new(e.clone())).eval(s);
                s.vars[*i] = VARS[*i].1.wrap(value);
            }
            Stmt::ShiftAssign(i, left, e) => {
                let value = Expr::Shift(*left, Box::new(Expr::Var(*i)), Box::new(e.clone()));
                s.vars[*i] = value.eval(s);
            }
            Stmt::Step(i, up) => {
                s.vars[*i] = VARS[*i].1.wrap(s.vars[*i] + if *up { 1 } else { -1 });
            }
            Stmt::ShowStep(i, up, post) => {
                let old = s.vars[*i];
                s.vars[*i] = VARS[*i].1.wrap(old + if *up { 1 } else { -1 });
                let shown = if *post { old } else { s.vars[*i] };
                let _ = write!(s.out, "{} ", U16.wrap(shown));
            }
            Stmt::Store(_, i, e) => {
                // The place is found before the value is computed, but
                // neither can change the other here.
                let at = (i.eval(s) & 7) as usize;
                s.ram16[at] = e.eval(s) as u16;
            }
            Stmt::Show(e) => {
                let _ = write!(s.out, "{} ", U16.wrap(e.eval(s)));
            }
            Stmt::If(c, then, otherwise) => {
                if c.eval(s) != 0 {
                    then.run(s);
                } else {
                    otherwise.run(s);
                }
            }
            Stmt::Repeat(n, body) => {
                for _ in 0..*n {
                    body.run(s);
                }
            }
        }
    }
}

/// A whole program around the statements, starting from `start`.
fn program(start: &State, stmts: &[Stmt]) -> String {
    use std::fmt::Write;
    let v = &start.vars;
    let mut text = String::from("#include \"test.h\"\n");
    for i in 0..4 {
        let (name, ty) = VARS[i];
        let _ = writeln!(text, "{} {name} = {};", ty.name(), v[i]);
    }
    let list = |values: &mut dyn Iterator<Item = String>| values.collect::<Vec<_>>().join(", ");
    let _ = writeln!(
        text,
        "const u8 rom8[16] = {{ {} }};",
        list(&mut ROM8.iter().map(|b| b.to_string()))
    );
    let _ = writeln!(
        text,
        "u16 ram16[8] = {{ {} }};",
        list(&mut start.ram16.iter().map(|w| w.to_string()))
    );
    text.push_str(
        "u8 id8(u8 x) { return x; }\n\
         u16 sum3(u16 a, u8 b, i16 c) { return a + b + c; }\n\
         i16 pick(u8 c, i16 a, i16 b) { if (c) return a; return b; }\n\
         void test(u8 p0, i8 p1, u16 p2, i16 p3) {\n",
    );
    for i in 4..8 {
        let (name, ty) = VARS[i];
        let _ = writeln!(text, "    {} {name} = {};", ty.name(), v[i]);
    }
    text.push_str("    u8* pb = rom8;\n    u16* pw = ram16;\n");
    for s in stmts {
        let _ = writeln!(text, "    {}", s.text());
    }
    // Everything the statements may have changed.
    for (name, _) in VARS {
        let _ = writeln!(text, "    show({name});");
    }
    text.push_str("    for (u8 k = 0; k < 8; k++) show(ram16[k]);\n}\n");
    let _ = writeln!(
        text,
        "void main() {{ test({}, {}, {}, {}); }}",
        v[8], v[9], v[10], v[11]
    );
    text
}

/// What the reference says the program prints.
fn expected(start: &State, stmts: &[Stmt]) -> String {
    use std::fmt::Write;
    let mut s = start.clone();
    for stmt in stmts {
        stmt.run(&mut s);
    }
    for v in s.vars {
        let _ = write!(s.out, "{} ", U16.wrap(v));
    }
    for w in s.ram16 {
        let _ = write!(s.out, "{w} ");
    }
    s.out
}

// ───────────────────────────── generators ─────────────────────────────

fn ty() -> impl Strategy<Value = Ty> {
    prop_oneof![Just(U8), Just(I8), Just(U16), Just(I16)]
}

/// Numbers, with the ones where things change (powers of two, the ends of
/// each type) more often than chance would give them.
fn number() -> impl Strategy<Value = i64> {
    let special = vec![
        0i64, 1, 2, 3, 4, 7, 8, 10, 15, 16, 32, 64, 100, 127, 128, 129, 255, 256, 257, 1000, 1024,
        4096, 32767, -1, -2, -127, -128, -129, -256, -32768,
    ];
    prop_oneof![
        any::<i16>().prop_map(i64::from),
        any::<i8>().prop_map(i64::from),
        prop::sample::select(special),
    ]
}

fn op() -> impl Strategy<Value = Op> {
    use Op::*;
    prop::sample::select(vec![Add, Sub, Mul, Div, Mod, And, Or, Xor])
}

fn rel() -> impl Strategy<Value = Rel> {
    use Rel::*;
    prop::sample::select(vec![Eq, Ne, Lt, Le, Gt, Ge])
}

fn var() -> impl Strategy<Value = usize> {
    0..VARS.len()
}

fn expr() -> impl Strategy<Value = Expr> {
    let leaf = prop_oneof![
        (ty(), number()).prop_map(|(ty, v)| Expr::Const(ty, ty.wrap(v))),
        var().prop_map(Expr::Var),
        var().prop_map(Expr::Var),
    ];
    leaf.prop_recursive(4, 20, 3, |inner| {
        let one = || inner.clone().prop_map(Box::new);
        prop_oneof![
            4 => (op(), one(), one()).prop_map(|(op, x, y)| Expr::Bin(op, x, y)),
            2 => (rel(), one(), one()).prop_map(|(rel, x, y)| Expr::Cmp(rel, x, y)),
            1 => (ty(), one()).prop_map(|(ty, x)| Expr::Cast(ty, x)),
            1 => (any::<bool>(), one(), one()).prop_map(|(left, x, n)| Expr::Shift(left, x, n)),
            1 => (any::<bool>(), one(), 0..18u8).prop_map(|(left, x, n)| Expr::ShiftBy(left, x, n)),
            1 => (one(), one()).prop_map(|(x, y)| Expr::And(x, y)),
            1 => (one(), one()).prop_map(|(x, y)| Expr::Or(x, y)),
            1 => one().prop_map(Expr::Not),
            1 => (one(), one(), one()).prop_map(|(c, x, y)| Expr::Cond(c, x, y)),
            1 => one().prop_map(Expr::Neg),
            1 => one().prop_map(Expr::Inv),
            1 => (any::<bool>(), one()).prop_map(|(pointer, i)| Expr::Rom8(pointer, i)),
            1 => (any::<bool>(), one()).prop_map(|(pointer, i)| Expr::Ram16(pointer, i)),
            1 => one().prop_map(Expr::Id8),
            1 => (one(), one(), one()).prop_map(|(x, y, z)| Expr::Sum3(x, y, z)),
            1 => (one(), one(), one()).prop_map(|(c, x, y)| Expr::Pick(c, x, y)),
        ]
    })
}

fn stmt() -> impl Strategy<Value = Stmt> {
    let simple = prop_oneof![
        4 => (var(), expr()).prop_map(|(i, e)| Stmt::Assign(i, e)),
        2 => (var(), op(), expr()).prop_map(|(i, op, e)| Stmt::OpAssign(i, op, e)),
        1 => (var(), any::<bool>(), expr()).prop_map(|(i, left, e)| Stmt::ShiftAssign(i, left, e)),
        1 => (var(), any::<bool>()).prop_map(|(i, up)| Stmt::Step(i, up)),
        1 => (var(), any::<bool>(), any::<bool>())
            .prop_map(|(i, up, post)| Stmt::ShowStep(i, up, post)),
        2 => (any::<bool>(), expr(), expr()).prop_map(|(pointer, i, e)| Stmt::Store(pointer, i, e)),
        3 => expr().prop_map(Stmt::Show),
    ];
    simple.prop_recursive(2, 6, 2, |inner| {
        let one = || inner.clone().prop_map(Box::new);
        prop_oneof![
            (expr(), one(), one()).prop_map(|(c, then, otherwise)| Stmt::If(c, then, otherwise)),
            (1..4u8, one()).prop_map(|(n, body)| Stmt::Repeat(n, body)),
        ]
    })
}

fn start() -> impl Strategy<Value = State> {
    (prop::array::uniform12(number()), any::<[u16; 8]>()).prop_map(|(values, ram16)| {
        let mut vars = [0; 12];
        for (i, v) in values.into_iter().enumerate() {
            vars[i] = VARS[i].1.wrap(v);
        }
        State {
            vars,
            ram16,
            out: String::new(),
        }
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1500))]

    #[test]
    fn compiled_programs_print_what_the_reference_computes(
        start in start(),
        stmts in prop::collection::vec(stmt(), 1..8),
    ) {
        let source = program(&start, &stmts);
        let printed = common::output(&source);
        prop_assert_eq!(printed, expected(&start, &stmts), "\n{}", source);
    }
}

/// The reference itself, on cases worked out by hand: a wrong reference
/// that agreed with a wrong compiler would prove nothing.
#[test]
fn the_reference_knows_the_rules() {
    let s = State {
        vars: [200, -3, 40000, -1234, 0, 0, 0, 0, 0, 0, 0, 0],
        ram16: [0; 8],
        out: String::new(),
    };
    let var = |i| Box::new(Expr::Var(i));
    let bin = |op, x, y| Expr::Bin(op, x, y).eval(&s);
    // u8 + i8 is done in u8.
    assert_eq!(bin(Op::Add, var(0), var(1)), 197);
    // u16 + i8: the i8 is widened with its sign.
    assert_eq!(bin(Op::Add, var(2), var(1)), 39997);
    // i16 / u16 is unsigned.
    assert_eq!(bin(Op::Div, var(3), var(2)), (65536 - 1234) / 40001);
    // i16 % i8 keeps the sign of the left operand.
    assert_eq!(bin(Op::Mod, var(3), var(1)), -1);
    assert_eq!(shift(I8, false, -128, 3), -16);
    assert_eq!(shift(I8, false, -128, 200), -1);
    assert_eq!(shift(U8, false, 128, 3), 16);
    assert_eq!(shift(U16, true, 0x8001, 1), 2);
    assert_eq!(shift(U16, true, 1, 16), 0);
    assert_eq!(
        Expr::Cmp(Rel::Lt, var(1), var(0)).eval(&s),
        0,
        "as u8, 253 < 200 is false"
    );
    assert_eq!(
        Expr::Cmp(Rel::Lt, var(3), var(1)).eval(&s),
        1,
        "as i16, -1234 < -3"
    );
    assert_eq!(Expr::Cast(U16, var(1)).eval(&s), 65533);
    assert_eq!(Expr::Cast(I8, var(0)).eval(&s), -56);
}
