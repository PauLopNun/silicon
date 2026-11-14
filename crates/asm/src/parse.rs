//! One line of assembly → statements.
//!
//! The instruction syntax is the one printed by `silicon_isa::Insn` and
//! listed in `docs/ISA.md`. Nothing about the encoding lives here: a parsed
//! instruction is an [`Insn`] whose numeric operand is still an expression.

use crate::expr::{Cursor, Expr};
use crate::lex::{LineError, Tok, Token};
use silicon_isa::{form, AluOp, Cond, Insn, Pair, Reg};

/// An expression and the column it starts at, for range errors.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Value {
    pub expr: Expr,
    pub col: u32,
}

/// One item of a `.byte` list.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Datum {
    Value(Value),
    Text(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Stmt {
    /// `name:`, with local names already attached to their scope.
    Label(String),
    /// `name = expr`
    Const(String, Expr),
    /// `.rom` (false) or `.ram` (true): continue where that part of memory
    /// was left.
    Section(bool),
    /// `.org addr`: continue at this address.
    Org(Value),
    /// `.res count`: leave room without writing anything (RAM variables).
    Res(Value),
    /// `.align n`: continue at the next multiple of `n`.
    Align(Value),
    /// `.fill count, value`
    Fill(Value, Option<Value>),
    /// `.byte a, "text", …`
    Byte(Vec<Datum>),
    /// `.word a, b, …`: 16 bits each, low byte first.
    Word(Vec<Value>),
    /// `.include "file"`
    Include(String),
    /// `.loc "file", line`: report the lines that follow as coming from
    /// there, one for one. Written by the compiler around assembly copied
    /// from its source.
    ///
    /// `.loc "file", line, col`: report everything that follows, until the
    /// next `.loc`, as that one place. Written by the compiler before the
    /// code it generates for a statement.
    Loc(String, u32, Option<u32>),
    /// A machine instruction. If its form has a numeric operand, the
    /// operand in the `Insn` is 0 and the real one is the expression.
    Insn(Insn, Option<Value>),
    /// `jz target` and friends: a conditional jump of any distance, encoded
    /// as a branch when the target is near and as a branch around a `jmp`
    /// when it is not.
    Jump(Cond, Value),
}

/// A statement and the column it starts at.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Located {
    pub stmt: Stmt,
    pub col: u32,
}

/// What kind of number an instruction carries after its opcode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Operand {
    None,
    /// `n`: one byte; −128…255 is accepted so that `ld a, -1` works.
    Byte,
    /// `[sp+n]`: 0…255.
    SpOffset,
    /// `d`: −128…127.
    Signed,
    /// `nn`: 16 bits.
    Word,
    /// A branch target; the encoded byte is its distance.
    Branch,
}

pub(crate) fn operand_of(insn: &Insn) -> Operand {
    use Insn::*;
    match insn {
        LdImm(..) | AluImm(..) => Operand::Byte,
        LeaSp(..) | LdSp(..) | StSp(..) => Operand::SpOffset,
        LeaPair(..) | LeaSpSp(_) => Operand::Signed,
        LdPairImm(..) | Jmp(_) | Call(_) | LdAAbs(_) | StAbsA(_) => Operand::Word,
        Br(..) => Operand::Branch,
        _ => Operand::None,
    }
}

/// The same instruction with its numeric operand replaced. The value must
/// already have been checked against [`operand_of`]'s range.
pub(crate) fn with_operand(insn: Insn, v: i64) -> Insn {
    use Insn::*;
    match insn {
        LdImm(r, _) => LdImm(r, v as u8),
        AluImm(op, _) => AluImm(op, v as u8),
        LeaSp(p, _) => LeaSp(p, v as u8),
        LdSp(r, _) => LdSp(r, v as u8),
        StSp(_, r) => StSp(v as u8, r),
        LeaPair(p, _) => LeaPair(p, v as i8),
        LeaSpSp(_) => LeaSpSp(v as i8),
        LdPairImm(p, _) => LdPairImm(p, v as u16),
        Jmp(_) => Jmp(v as u16),
        Call(_) => Call(v as u16),
        LdAAbs(_) => LdAAbs(v as u16),
        StAbsA(_) => StAbsA(v as u16),
        Br(c, _) => Br(c, v as i8),
        other => other,
    }
}

/// Parse the tokens of one line. `scope` is the last global label seen; it
/// is updated when the line defines a new one.
pub(crate) fn parse_line(
    toks: &[Token],
    line_len: usize,
    scope: &mut String,
) -> Result<Vec<Located>, LineError> {
    let mut cur = Cursor::new(toks, line_len);
    let mut out = Vec::new();

    // Any number of labels, then at most one statement.
    while let (Some(Tok::Ident(name)), Some(Tok::Punct(':'))) = (cur.peek(), cur.peek_at(1)) {
        let col = cur.col();
        let full = define(name, scope, col)?;
        // Only a plain name begins a new scope. `first.loop:` is a local
        // label of `first` spelled in full, wherever it is written, and
        // the local labels after it still belong to the label above.
        if !name.contains('.') {
            scope.clone_from(&full);
        }
        cur.advance();
        cur.advance();
        out.push(Located {
            stmt: Stmt::Label(full),
            col,
        });
    }
    if cur.at_end() {
        return Ok(out);
    }

    let col = cur.col();
    let Some(Tok::Ident(name)) = cur.peek() else {
        return Err((
            col,
            "expected an instruction, a label or a directive".into(),
        ));
    };
    cur.advance();
    let stmt = if cur.eat('=') {
        let full = define(name, scope, col)?;
        Stmt::Const(full, Expr::parse(&mut cur, scope)?)
    } else if name.starts_with('.') {
        directive(name, col, &mut cur, scope)?
    } else {
        instruction(name, col, &mut cur, scope)?
    };
    if !cur.at_end() {
        return Err((cur.col(), "expected the end of the line here".into()));
    }
    out.push(Located { stmt, col });
    Ok(out)
}

/// The full name of a symbol being defined: `.loop` after `main:` is
/// `main.loop`.
fn define(name: &str, scope: &str, col: u32) -> Result<String, LineError> {
    if crate::expr::is_register(name) {
        return Err((col, format!("`{name}` is a register and cannot be a name")));
    }
    if !name.starts_with('.') {
        return Ok(name.to_string());
    }
    if scope.is_empty() {
        return Err((col, format!("local label `{name}` has no label before it")));
    }
    Ok(format!("{scope}{name}"))
}

fn value(cur: &mut Cursor, scope: &str) -> Result<Value, LineError> {
    let col = cur.col();
    Ok(Value {
        expr: Expr::parse(cur, scope)?,
        col,
    })
}

fn string(cur: &mut Cursor, what: &str) -> Result<String, LineError> {
    let col = cur.col();
    match cur.peek() {
        Some(Tok::Str(bytes)) => {
            cur.advance();
            Ok(String::from_utf8_lossy(bytes).into_owned())
        }
        _ => Err((col, format!("expected {what} in double quotes"))),
    }
}

fn directive(name: &str, col: u32, cur: &mut Cursor, scope: &str) -> Result<Stmt, LineError> {
    Ok(match name.to_ascii_lowercase().as_str() {
        ".rom" => Stmt::Section(false),
        ".ram" => Stmt::Section(true),
        ".org" => Stmt::Org(value(cur, scope)?),
        ".res" => Stmt::Res(value(cur, scope)?),
        ".align" => Stmt::Align(value(cur, scope)?),
        ".fill" => {
            let count = value(cur, scope)?;
            let byte = if cur.eat(',') {
                Some(value(cur, scope)?)
            } else {
                None
            };
            Stmt::Fill(count, byte)
        }
        ".byte" => {
            let mut data = Vec::new();
            loop {
                if let Some(Tok::Str(bytes)) = cur.peek() {
                    cur.advance();
                    data.push(Datum::Text(bytes.clone()));
                } else {
                    data.push(Datum::Value(value(cur, scope)?));
                }
                if !cur.eat(',') {
                    break;
                }
            }
            Stmt::Byte(data)
        }
        ".word" => {
            let mut data = vec![value(cur, scope)?];
            while cur.eat(',') {
                data.push(value(cur, scope)?);
            }
            Stmt::Word(data)
        }
        ".include" => Stmt::Include(string(cur, "a file name")?),
        ".loc" => {
            let file = string(cur, "a file name")?;
            cur.expect(',')?;
            let line_col = cur.col();
            let line = match cur.peek() {
                Some(Tok::Num(n)) if (1..=u32::MAX as i64).contains(n) => *n as u32,
                _ => return Err((line_col, "expected a line number".into())),
            };
            cur.advance();
            let col = if cur.eat(',') {
                let col_col = cur.col();
                match cur.peek() {
                    Some(Tok::Num(n)) if (1..=u32::MAX as i64).contains(n) => {
                        cur.advance();
                        Some(*n as u32)
                    }
                    _ => return Err((col_col, "expected a column number".into())),
                }
            } else {
                None
            };
            Stmt::Loc(file, line, col)
        }
        _ => return Err((col, format!("unknown directive `{name}`"))),
    })
}

/// An operand as written, before deciding which instruction it belongs to.
#[derive(Clone, Debug, PartialEq)]
enum Arg {
    Reg(Reg),
    Pair(Pair),
    Sp,
    /// A bare expression.
    Num(Value),
    /// `[pp]`
    At(Pair),
    /// `[pp+]`
    AtInc(Pair),
    /// `[pp+a]`
    AtIdx(Pair),
    /// `[pp+d]`, `[pp-d]`
    AtOff(Pair, Value),
    /// `[sp+n]`, `[sp-n]`, `[sp]`
    AtSp(Value),
    /// `[nn]`
    AtAbs(Value),
}

fn reg_named(name: &str) -> Option<Reg> {
    let lower = name.to_ascii_lowercase();
    let i = Reg::NAMES.iter().position(|n| *n == lower)?;
    Some(Reg::ALL[i])
}

fn pair_named(name: &str) -> Option<Pair> {
    let lower = name.to_ascii_lowercase();
    let i = Pair::NAMES.iter().position(|n| *n == lower)?;
    Some(Pair::ALL[i])
}

fn is_sp(name: &str) -> bool {
    name.eq_ignore_ascii_case("sp")
}

fn arg(cur: &mut Cursor, scope: &str) -> Result<Arg, LineError> {
    match cur.peek() {
        Some(Tok::Ident(name)) if is_sp(name) => {
            cur.advance();
            Ok(Arg::Sp)
        }
        Some(Tok::Ident(name)) if reg_named(name).is_some() => {
            cur.advance();
            Ok(Arg::Reg(reg_named(name).expect("checked")))
        }
        Some(Tok::Ident(name)) if pair_named(name).is_some() => {
            cur.advance();
            Ok(Arg::Pair(pair_named(name).expect("checked")))
        }
        Some(Tok::Punct('[')) => {
            cur.advance();
            let inside = address(cur, scope)?;
            cur.expect(']')?;
            Ok(inside)
        }
        _ => Ok(Arg::Num(value(cur, scope)?)),
    }
}

/// What stands between `[` and `]`.
fn address(cur: &mut Cursor, scope: &str) -> Result<Arg, LineError> {
    let col = cur.col();
    let Some(Tok::Ident(name)) = cur.peek() else {
        return Ok(Arg::AtAbs(value(cur, scope)?));
    };
    let base = pair_named(name);
    if base.is_none() && !is_sp(name) {
        if reg_named(name).is_some() {
            return Err((
                col,
                format!("`{name}` is 8 bits wide; an address needs a pair (`ba`, `x`, `y`, `z`)"),
            ));
        }
        return Ok(Arg::AtAbs(value(cur, scope)?));
    }
    cur.advance();

    // `+]` and `+a]` are the two forms that are not an offset.
    let plus = cur.peek() == Some(&Tok::Punct('+'));
    let after = cur.peek_at(1);
    let close = Some(&Tok::Punct(']'));
    if let Some(p) = base {
        if plus && after == close {
            cur.advance();
            return Ok(Arg::AtInc(p));
        }
        let indexed = matches!(after, Some(Tok::Ident(r)) if r.eq_ignore_ascii_case("a"));
        if plus && indexed && cur.peek_at(2) == close {
            cur.advance();
            cur.advance();
            return Ok(Arg::AtIdx(p));
        }
    }
    let offset = match cur.peek() {
        Some(Tok::Punct(']')) if base.is_some() => return Ok(Arg::At(base.expect("checked"))),
        Some(Tok::Punct(']')) => Value {
            expr: Expr::Num(0),
            col,
        },
        Some(Tok::Punct('+')) => {
            cur.advance();
            value(cur, scope)?
        }
        // The minus sign is the start of the offset: `[x-1]` is `x + (-1)`.
        Some(Tok::Punct('-')) => value(cur, scope)?,
        _ => return Err((cur.col(), "expected `]`, `+` or `-`".into())),
    };
    Ok(match base {
        Some(p) => Arg::AtOff(p, offset),
        None => Arg::AtSp(offset),
    })
}

/// Mnemonics of the conditional jumps of any distance, by condition number.
const JUMPS: [&str; 8] = ["jz", "jnz", "jc", "jnc", "jn", "jp", "jlt", "jge"];

fn instruction(name: &str, col: u32, cur: &mut Cursor, scope: &str) -> Result<Stmt, LineError> {
    let m = name.to_ascii_lowercase();
    let alu = AluOp::NAMES
        .iter()
        .position(|n| *n == m)
        .map(|i| AluOp::ALL[i]);
    let branch = Cond::NAMES
        .iter()
        .position(|n| *n == m)
        .map(|i| Cond::ALL[i]);
    let jump = JUMPS.iter().position(|n| *n == m).map(|i| Cond::ALL[i]);
    const PLAIN: [&str; 15] = [
        "hlt", "ret", "nop", "mov", "jmp", "call", "ld", "st", "push", "pop", "lea", "inc", "dec",
        "shr", "ror",
    ];
    if alu.is_none() && branch.is_none() && jump.is_none() && !PLAIN.contains(&&*m) && m != "asr" {
        return Err((col, format!("unknown instruction `{name}`")));
    }

    let mut args = Vec::new();
    if !cur.at_end() {
        loop {
            args.push(arg(cur, scope)?);
            if !cur.eat(',') {
                break;
            }
        }
    }

    use Arg::*;
    let plain = |insn| Some((insn, None));
    let with = |insn, v: &Value| Some((insn, Some(v.clone())));
    let a = silicon_isa::Reg::A;
    let parsed = match (m.as_str(), args.as_slice()) {
        ("hlt", []) => plain(Insn::Hlt),
        ("ret", []) => plain(Insn::Ret),
        ("nop", []) => plain(Insn::NOP),
        ("mov", [Sp, Pair(silicon_isa::Pair::X)]) => plain(Insn::MovSpX),
        ("mov", [Reg(d), Reg(s)]) => plain(Insn::Mov(*d, *s)),
        ("jmp", [Pair(silicon_isa::Pair::Z)]) => plain(Insn::JmpZ),
        ("jmp", [Num(v)]) => with(Insn::Jmp(0), v),
        ("call", [Num(v)]) => with(Insn::Call(0), v),
        ("ld", [Reg(r), At(p)]) if *r == a => plain(Insn::LdAPtr(*p)),
        ("ld", [Reg(r), AtInc(p)]) if *r == a => plain(Insn::LdAPtrInc(*p)),
        ("ld", [Reg(r), AtIdx(p)]) if *r == a => plain(Insn::LdAIdx(*p)),
        ("ld", [Reg(r), AtAbs(v)]) if *r == a => with(Insn::LdAAbs(0), v),
        ("ld", [Reg(r), AtSp(v)]) => with(Insn::LdSp(*r, 0), v),
        ("ld", [Reg(r), Num(v)]) => with(Insn::LdImm(*r, 0), v),
        ("ld", [Pair(p), Num(v)]) => with(Insn::LdPairImm(*p, 0), v),
        ("st", [At(p), Reg(r)]) if *r == a => plain(Insn::StPtrA(*p)),
        ("st", [AtInc(p), Reg(r)]) if *r == a => plain(Insn::StPtrIncA(*p)),
        ("st", [AtAbs(v), Reg(r)]) if *r == a => with(Insn::StAbsA(0), v),
        ("st", [AtSp(v), Reg(r)]) => with(Insn::StSp(0, *r), v),
        ("push", [Pair(p)]) => plain(Insn::Push(*p)),
        ("pop", [Pair(p)]) => plain(Insn::Pop(*p)),
        ("lea", [Pair(p), AtSp(v)]) => with(Insn::LeaSp(*p, 0), v),
        ("lea", [Pair(p), AtOff(q, v)]) if p == q => with(Insn::LeaPair(*p, 0), v),
        ("lea", [Pair(p), AtIdx(q)]) if p == q => plain(Insn::LeaIdx(*p)),
        ("lea", [Sp, AtSp(v)]) => with(Insn::LeaSpSp(0), v),
        ("add", [Pair(d), Pair(s)]) => plain(Insn::AddPair(*d, *s)),
        ("inc", [Reg(r)]) => plain(Insn::Inc(*r)),
        ("dec", [Reg(r)]) => plain(Insn::Dec(*r)),
        ("shr", [Reg(r)]) if *r == a => plain(Insn::ShrA),
        ("ror", [Reg(r)]) if *r == a => plain(Insn::RorA),
        ("asr", [Reg(r)]) if *r == a => plain(Insn::AsrA),
        (_, [Reg(r), Reg(s)]) if alu.is_some() && *r == a => {
            plain(Insn::Alu(alu.expect("checked"), *s))
        }
        (_, [Reg(r), Num(v)]) if alu.is_some() && *r == a => {
            with(Insn::AluImm(alu.expect("checked"), 0), v)
        }
        (_, [Num(v)]) if branch.is_some() => with(Insn::Br(branch.expect("checked"), 0), v),
        (_, [Num(v)]) if jump.is_some() => {
            return Ok(Stmt::Jump(jump.expect("checked"), v.clone()));
        }
        _ => None,
    };
    match parsed {
        Some((insn, operand)) => Ok(Stmt::Insn(insn, operand)),
        None => Err((
            col,
            format!(
                "`{m}` does not take these operands; it is written {}",
                forms_of(&m)
            ),
        )),
    }
}

/// The ways an instruction can be written, taken from the opcode table.
fn forms_of(mnemonic: &str) -> String {
    if JUMPS.contains(&mnemonic) {
        return format!("`{mnemonic} target`");
    }
    if mnemonic == "nop" {
        return "`nop`".into();
    }
    let is_alu = AluOp::NAMES.contains(&mnemonic);
    let is_branch = Cond::NAMES.contains(&mnemonic);
    let mut forms = Vec::new();
    for f in form::FORMS {
        let (head, rest) = f.syntax.split_once(' ').unwrap_or((f.syntax, ""));
        let shown = match head {
            "op" if is_alu => format!("{mnemonic} {rest}"),
            "bcc" if is_branch => format!("{mnemonic} target"),
            _ if head == mnemonic => f.syntax.to_string(),
            _ => continue,
        };
        forms.push(format!("`{shown}`"));
    }
    forms.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::lex;

    fn parse(line: &str) -> Result<Vec<Stmt>, LineError> {
        let mut scope = String::from("main");
        let toks = lex(line)?;
        Ok(parse_line(&toks, line.len(), &mut scope)?
            .into_iter()
            .map(|l| l.stmt)
            .collect())
    }

    fn insn(line: &str) -> Insn {
        match parse(line).unwrap().as_slice() {
            [Stmt::Insn(insn, _)] => *insn,
            other => panic!("{line:?} parsed as {other:?}"),
        }
    }

    #[test]
    fn every_addressing_form_is_recognised() {
        use silicon_isa::Pair::*;
        use Insn::*;
        assert_eq!(insn("ld a, [x]"), LdAPtr(X));
        assert_eq!(insn("ld a, [ba+]"), LdAPtrInc(Ba));
        assert_eq!(insn("ld a, [y+a]"), LdAIdx(Y));
        assert_eq!(insn("ld a, [0x8800]"), LdAAbs(0));
        assert_eq!(insn("ld zh, [sp+4]"), LdSp(Reg::Zh, 0));
        assert_eq!(insn("ld b, [sp]"), LdSp(Reg::B, 0));
        assert_eq!(insn("ld b, 7"), LdImm(Reg::B, 0));
        assert_eq!(insn("ld z, table"), LdPairImm(Z, 0));
        assert_eq!(insn("st [z], a"), StPtrA(Z));
        assert_eq!(insn("st [x+], a"), StPtrIncA(X));
        assert_eq!(insn("st [sp+1], xl"), StSp(0, Reg::Xl));
        assert_eq!(insn("st [var], a"), StAbsA(0));
        assert_eq!(insn("lea x, [sp+2]"), LeaSp(X, 0));
        assert_eq!(insn("lea y, [y-1]"), LeaPair(Y, 0));
        assert_eq!(insn("lea y, [y+a]"), LeaIdx(Y));
        assert_eq!(insn("lea sp, [sp-8]"), LeaSpSp(0));
        assert_eq!(insn("add x, ba"), AddPair(X, Ba));
        assert_eq!(insn("add a, b"), Alu(AluOp::Add, Reg::B));
        assert_eq!(insn("CMP A, 3"), AluImm(AluOp::Cmp, 0));
        assert_eq!(insn("mov sp, x"), MovSpX);
        assert_eq!(insn("jmp z"), JmpZ);
        assert_eq!(insn("jmp main"), Jmp(0));
        assert_eq!(insn("nop"), Insn::NOP);
        assert_eq!(insn("bge .loop"), Br(Cond::Ge, 0));
    }

    #[test]
    fn labels_constants_and_scopes() {
        assert_eq!(
            parse("draw: .row: inc a").unwrap(),
            [
                Stmt::Label("draw".into()),
                Stmt::Label("draw.row".into()),
                Stmt::Insn(Insn::Inc(Reg::A), None)
            ]
        );
        assert_eq!(
            parse(".again:").unwrap(),
            [Stmt::Label("main.again".into())]
        );
        assert_eq!(
            parse("width = 128").unwrap(),
            [Stmt::Const("width".into(), Expr::Num(128))]
        );
        assert!(matches!(
            parse("jnz .again").unwrap().as_slice(),
            [Stmt::Jump(Cond::Nz, v)] if v.expr == Expr::Sym("main.again".into(), 5)
        ));
    }

    #[test]
    fn directives() {
        assert!(matches!(
            parse(".org 0x9000").unwrap().as_slice(),
            [Stmt::Org(_)]
        ));
        assert_eq!(parse(".ram").unwrap(), [Stmt::Section(true)]);
        assert_eq!(parse(".ROM").unwrap(), [Stmt::Section(false)]);
        assert!(matches!(
            parse(".byte 1, \"hi\", 'c'").unwrap().as_slice(),
            [Stmt::Byte(d)] if d.len() == 3 && d[1] == Datum::Text(b"hi".to_vec())
        ));
        assert!(matches!(parse(".word 1, 2").unwrap().as_slice(), [Stmt::Word(w)] if w.len() == 2));
        assert!(matches!(
            parse(".fill 4").unwrap().as_slice(),
            [Stmt::Fill(_, None)]
        ));
        assert!(matches!(
            parse(".fill 4, 0xff").unwrap().as_slice(),
            [Stmt::Fill(_, Some(_))]
        ));
        assert_eq!(
            parse(".loc \"a.c\", 12, 5").unwrap(),
            [Stmt::Loc("a.c".into(), 12, Some(5))]
        );
        assert_eq!(
            parse(".include \"lib/io.asm\"").unwrap(),
            [Stmt::Include("lib/io.asm".into())]
        );
        assert_eq!(
            parse(".loc \"a.c\", 12").unwrap(),
            [Stmt::Loc("a.c".into(), 12, None)]
        );
    }

    #[test]
    fn errors_say_what_is_wrong_and_where() {
        let err = |line: &str| parse(line).unwrap_err();
        assert_eq!(err("  lod a, 1"), (3, "unknown instruction `lod`".into()));
        assert_eq!(err(".bytes 1"), (1, "unknown directive `.bytes`".into()));
        assert_eq!(
            err("inc a b"),
            (7, "expected the end of the line here".into())
        );
        assert_eq!(err("ld a, [x+1"), (11, "expected `]`".into()));
        assert_eq!(err("ld a, [x"), (9, "expected `]`, `+` or `-`".into()));
        assert_eq!(err("ld a, [x*2]"), (9, "expected `]`, `+` or `-`".into()));
        assert_eq!(
            err("a: inc a"),
            (1, "`a` is a register and cannot be a name".into())
        );
        assert_eq!(err("sp = 3").0, 1);
        assert_eq!(
            err(".include lib").1,
            "expected a file name in double quotes"
        );
        assert!(err("ld a, [b]").1.contains("8 bits wide"));
        // The forms come from the opcode table.
        assert_eq!(
            err("ld b, [x]"),
            (
                1,
                "`ld` does not take these operands; it is written `ld a, [pp]`, `ld a, [pp+]`, \
                 `ld a, [pp+a]`, `ld pp, nn`, `ld r, n`, `ld r, [sp+n]`, `ld a, [nn]`"
                    .into()
            )
        );
        assert_eq!(
            err("xor b, a").1,
            "`xor` does not take these operands; it is written `xor a, rs`, `xor a, n`"
        );
        assert_eq!(
            err("lea x, [y+1]").1,
            "`lea` does not take these operands; it is written `lea pp, [sp+n]`, \
             `lea pp, [pp+d]`, `lea pp, [pp+a]`, `lea sp, [sp+d]`"
        );
        assert!(err("bz").1.ends_with("it is written `bz target`"));
        assert!(err("jz a").1.ends_with("it is written `jz target`"));
    }
}
