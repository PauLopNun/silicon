//! Expressions: numbers, symbols and arithmetic, with C-like precedence.
//!
//! ```text
//!   |   ^   &   << >>   + -   * / %   unary - ~   ( )   lo(e) hi(e)   $
//!   lowest ───────────────────────────────────────────────► highest
//! ```

use crate::lex::{LineError, Tok, Token};
use silicon_isa::{Pair, Reg};

/// A read position in the tokens of one line.
pub(crate) struct Cursor<'a> {
    toks: &'a [Token],
    pos: usize,
    /// Column just past the end of the line, for "something is missing here".
    end: u32,
}

impl<'a> Cursor<'a> {
    pub fn new(toks: &'a [Token], line_len: usize) -> Self {
        Cursor {
            toks,
            pos: 0,
            end: line_len as u32 + 1,
        }
    }

    pub fn peek(&self) -> Option<&'a Tok> {
        self.toks.get(self.pos).map(|t| &t.tok)
    }

    pub fn peek_at(&self, ahead: usize) -> Option<&'a Tok> {
        self.toks.get(self.pos + ahead).map(|t| &t.tok)
    }

    /// Column of the next token, or of the end of the line.
    pub fn col(&self) -> u32 {
        self.toks.get(self.pos).map_or(self.end, |t| t.col)
    }

    pub fn advance(&mut self) {
        self.pos += 1;
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.toks.len()
    }

    /// Consume `c` if it is next.
    pub fn eat(&mut self, c: char) -> bool {
        let hit = self.peek() == Some(&Tok::Punct(c));
        if hit {
            self.pos += 1;
        }
        hit
    }

    pub fn expect(&mut self, c: char) -> Result<(), LineError> {
        if self.eat(c) {
            Ok(())
        } else {
            Err((self.col(), format!("expected `{c}`")))
        }
    }
}

/// Is this name a register, and so not usable as a symbol?
pub(crate) fn is_register(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "sp" || Reg::NAMES.contains(&lower.as_str()) || Pair::NAMES.contains(&lower.as_str())
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Expr {
    Num(i64),
    /// A symbol and the column it was written at.
    Sym(String, u32),
    /// `$`: the address of the current statement.
    Here,
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Lo(Box<Expr>),
    Hi(Box<Expr>),
    /// A binary operation and the column of its operator.
    Binary(Op, u32, Box<Expr>, Box<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Op {
    Or,
    Xor,
    And,
    Shl,
    Shr,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

fn binary_op(tok: &Tok) -> Option<(Op, u8)> {
    Some(match tok {
        Tok::Punct('|') => (Op::Or, 1),
        Tok::Punct('^') => (Op::Xor, 2),
        Tok::Punct('&') => (Op::And, 3),
        Tok::Shl => (Op::Shl, 4),
        Tok::Shr => (Op::Shr, 4),
        Tok::Punct('+') => (Op::Add, 5),
        Tok::Punct('-') => (Op::Sub, 5),
        Tok::Punct('*') => (Op::Mul, 6),
        Tok::Punct('/') => (Op::Div, 6),
        Tok::Punct('%') => (Op::Rem, 6),
        _ => return None,
    })
}

impl Expr {
    /// Parse an expression. `scope` is the enclosing global label, which
    /// `.local` names are attached to.
    pub fn parse(cur: &mut Cursor, scope: &str) -> Result<Expr, LineError> {
        Expr::binary(cur, scope, 1)
    }

    fn binary(cur: &mut Cursor, scope: &str, min_level: u8) -> Result<Expr, LineError> {
        let mut left = Expr::unary(cur, scope)?;
        while let Some((op, level)) = cur.peek().and_then(binary_op) {
            if level < min_level {
                break;
            }
            let col = cur.col();
            cur.advance();
            let right = Expr::binary(cur, scope, level + 1)?;
            left = Expr::Binary(op, col, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(cur: &mut Cursor, scope: &str) -> Result<Expr, LineError> {
        let col = cur.col();
        let Some(tok) = cur.peek() else {
            return Err((col, "expected a value".into()));
        };
        cur.advance();
        Ok(match tok {
            Tok::Num(n) => Expr::Num(*n),
            Tok::Punct('$') => Expr::Here,
            Tok::Punct('-') => Expr::Neg(Box::new(Expr::unary(cur, scope)?)),
            Tok::Punct('+') => Expr::unary(cur, scope)?,
            Tok::Punct('~') => Expr::Not(Box::new(Expr::unary(cur, scope)?)),
            Tok::Punct('(') => {
                let inner = Expr::parse(cur, scope)?;
                cur.expect(')')?;
                inner
            }
            Tok::Ident(name) => {
                let function = name.to_ascii_lowercase();
                if (function == "lo" || function == "hi") && cur.eat('(') {
                    let inner = Box::new(Expr::parse(cur, scope)?);
                    cur.expect(')')?;
                    if function == "lo" {
                        Expr::Lo(inner)
                    } else {
                        Expr::Hi(inner)
                    }
                } else if is_register(name) {
                    return Err((col, format!("`{name}` is a register, not a value")));
                } else if name.starts_with('.') {
                    if scope.is_empty() {
                        return Err((col, format!("local label `{name}` has no label before it")));
                    }
                    Expr::Sym(format!("{scope}{name}"), col)
                } else {
                    Expr::Sym(name.clone(), col)
                }
            }
            Tok::Str(_) => return Err((col, "a string is not a value here".into())),
            _ => return Err((col, "expected a value".into())),
        })
    }

    /// Does the value depend on where the expression is written (`$`)?
    pub fn mentions_here(&self) -> bool {
        match self {
            Expr::Here => true,
            Expr::Num(_) | Expr::Sym(..) => false,
            Expr::Neg(e) | Expr::Not(e) | Expr::Lo(e) | Expr::Hi(e) => e.mentions_here(),
            Expr::Binary(_, _, l, r) => l.mentions_here() || r.mentions_here(),
        }
    }

    /// Compute the value. `here` is the address of the statement, and
    /// `lookup` gives the value of a symbol or explains why it has none.
    pub fn eval(
        &self,
        here: i64,
        lookup: &mut dyn FnMut(&str) -> Result<i64, String>,
    ) -> Result<i64, LineError> {
        Ok(match self {
            Expr::Num(n) => *n,
            Expr::Here => here,
            Expr::Sym(name, col) => lookup(name).map_err(|message| (*col, message))?,
            Expr::Neg(e) => e.eval(here, lookup)?.wrapping_neg(),
            Expr::Not(e) => !e.eval(here, lookup)?,
            Expr::Lo(e) => e.eval(here, lookup)? & 0xFF,
            Expr::Hi(e) => e.eval(here, lookup)? >> 8 & 0xFF,
            Expr::Binary(op, col, l, r) => {
                let (l, r) = (l.eval(here, lookup)?, r.eval(here, lookup)?);
                match op {
                    Op::Or => l | r,
                    Op::Xor => l ^ r,
                    Op::And => l & r,
                    Op::Add => l.wrapping_add(r),
                    Op::Sub => l.wrapping_sub(r),
                    Op::Mul => l.wrapping_mul(r),
                    Op::Shl | Op::Shr if !(0..64).contains(&r) => {
                        return Err((*col, format!("cannot shift by {r}")));
                    }
                    Op::Shl => l << r,
                    Op::Shr => l >> r,
                    Op::Div | Op::Rem if r == 0 => {
                        return Err((*col, "division by zero".into()));
                    }
                    Op::Div => l.wrapping_div(r),
                    Op::Rem => l.wrapping_rem(r),
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::lex;

    fn value(text: &str) -> Result<i64, LineError> {
        let toks = lex(text).unwrap();
        let mut cur = Cursor::new(&toks, text.len());
        let expr = Expr::parse(&mut cur, "main")?;
        assert!(cur.at_end(), "trailing tokens in {text:?}");
        expr.eval(0x100, &mut |name| match name {
            "ten" => Ok(10),
            "main.loop" => Ok(0x1234),
            other => Err(format!("`{other}` is not defined")),
        })
    }

    #[test]
    fn arithmetic_follows_c_precedence() {
        assert_eq!(value("1 + 2 * 3"), Ok(7));
        assert_eq!(value("(1 + 2) * 3"), Ok(9));
        assert_eq!(value("10 - 4 - 3"), Ok(3), "left associative");
        assert_eq!(value("1 << 4 | 1"), Ok(17));
        assert_eq!(value("0xF0 & 0x3C ^ 0xFF"), Ok(0xCF));
        assert_eq!(value("7 / 2 + 7 % 2"), Ok(4));
        assert_eq!(value("-ten + 3"), Ok(-7));
        assert_eq!(value("~0 & 0xFF"), Ok(255));
        assert_eq!(value("- -5"), Ok(5));
    }

    #[test]
    fn symbols_here_and_byte_selectors() {
        assert_eq!(value("ten * ten"), Ok(100));
        assert_eq!(value("$ + 2"), Ok(0x102));
        assert_eq!(value(".loop"), Ok(0x1234), "local label in scope `main`");
        assert_eq!(value("lo(.loop)"), Ok(0x34));
        assert_eq!(value("hi(.loop) + 1"), Ok(0x13));
        assert_eq!(value("LO(0xABCD)"), Ok(0xCD));
    }

    #[test]
    fn errors_carry_the_column() {
        assert_eq!(value("1 + nope"), Err((5, "`nope` is not defined".into())));
        assert_eq!(value("1 / (ten - 10)"), Err((3, "division by zero".into())));
        assert_eq!(value("1 +"), Err((4, "expected a value".into())));
        assert_eq!(value("(1 + 2"), Err((7, "expected `)`".into())));
        assert_eq!(
            value("xl + 1"),
            Err((1, "`xl` is a register, not a value".into()))
        );
        assert_eq!(value("1 << 99").unwrap_err().0, 3);
    }
}
