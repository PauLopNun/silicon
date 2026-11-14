//! Tokens → tree, by recursive descent. The grammar is a small subset of
//! C's; `docs/LANGUAGE.md` describes it.

use crate::ast::*;
use crate::lex::{Loc, Tok, Token};

/// A syntax error: where, and what was expected.
pub(crate) type Error = (Loc, String);
type Result<T> = std::result::Result<T, Error>;

/// Words that cannot be names.
const KEYWORDS: [&str; 17] = [
    "u8", "i8", "u16", "i16", "void", "const", "if", "else", "while", "do", "for", "return",
    "break", "continue", "sizeof", "asm", "include",
];

pub(crate) struct Parser<'a> {
    toks: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    pub fn new(toks: &'a [Token]) -> Self {
        Parser { toks, pos: 0 }
    }

    fn peek(&self) -> &'a Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, ahead: usize) -> &'a Tok {
        &self.toks[(self.pos + ahead).min(self.toks.len() - 1)].tok
    }

    fn loc(&self) -> Loc {
        self.toks[self.pos].loc
    }

    fn advance(&mut self) {
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
    }

    fn at(&self, p: &str) -> bool {
        matches!(self.peek(), Tok::Punct(q) if *q == p)
    }

    fn at_word(&self, word: &str) -> bool {
        matches!(self.peek(), Tok::Ident(w) if w == word)
    }

    fn eat(&mut self, p: &str) -> bool {
        let hit = self.at(p);
        if hit {
            self.advance();
        }
        hit
    }

    fn eat_word(&mut self, word: &str) -> bool {
        let hit = self.at_word(word);
        if hit {
            self.advance();
        }
        hit
    }

    fn expect(&mut self, p: &str) -> Result<()> {
        if self.eat(p) {
            Ok(())
        } else {
            Err((
                self.loc(),
                format!("expected `{p}` but found {}", self.found()),
            ))
        }
    }

    /// The next token, as an error message names it.
    fn found(&self) -> String {
        match self.peek() {
            Tok::Ident(w) => format!("`{w}`"),
            Tok::Num(n) => format!("`{n}`"),
            Tok::Str(_) => "a string".into(),
            Tok::Punct(p) => format!("`{p}`"),
            Tok::Asm(_) => "an `asm` block".into(),
            Tok::Eof => "the end of the file".into(),
        }
    }

    fn name(&mut self, what: &str) -> Result<(String, Loc)> {
        let loc = self.loc();
        match self.peek() {
            Tok::Ident(w) if !KEYWORDS.contains(&w.as_str()) => {
                self.advance();
                Ok((w.clone(), loc))
            }
            Tok::Ident(w) => Err((loc, format!("`{w}` is a keyword and cannot be {what}"))),
            _ => Err((loc, format!("expected {what} but found {}", self.found()))),
        }
    }

    // ───────────────────────────── types ─────────────────────────────

    fn base(&self) -> Option<Base> {
        let Tok::Ident(word) = self.peek() else {
            return None;
        };
        Some(match word.as_str() {
            "u8" => Base::U8,
            "i8" => Base::I8,
            "u16" => Base::U16,
            "i16" => Base::I16,
            "void" => Base::Void,
            _ => return None,
        })
    }

    /// Does a type begin here?
    fn at_type(&self) -> bool {
        self.base().is_some() || self.at_word("const")
    }

    /// `const`? base `*`*. Returns whether `const` was written.
    fn type_name(&mut self) -> Result<(TypeName, bool)> {
        let is_const = self.eat_word("const");
        let Some(base) = self.base() else {
            return Err((
                self.loc(),
                format!(
                    "expected a type (`u8`, `i8`, `u16`, `i16`, `void`) but found {}",
                    self.found()
                ),
            ));
        };
        self.advance();
        let mut pointers = 0;
        while self.eat("*") {
            pointers += 1;
        }
        Ok((TypeName { base, pointers }, is_const))
    }

    // ─────────────────────────── top level ───────────────────────────

    pub fn program(&mut self) -> Result<Vec<Item>> {
        let mut items = Vec::new();
        while *self.peek() != Tok::Eof {
            let loc = self.loc();
            if let Tok::Asm(text) = self.peek() {
                self.advance();
                items.push(Item::Asm(text.clone(), loc));
                continue;
            }
            let (ty, is_const) = self.type_name()?;
            if matches!(self.peek_at(1), Tok::Punct("(")) {
                items.push(Item::Function(self.function(ty)?));
            } else {
                for decl in self.declarators(ty, is_const)? {
                    items.push(Item::Global(decl));
                }
            }
        }
        Ok(items)
    }

    fn function(&mut self, ret: TypeName) -> Result<Function> {
        let (name, loc) = self.name("the name of a function")?;
        self.expect("(")?;
        let mut params = Vec::new();
        // `f(void)` and `f()` both mean "no parameters".
        let empty = self.at_word("void") && matches!(self.peek_at(1), Tok::Punct(")"));
        if empty {
            self.advance();
        }
        while !self.at(")") {
            let (ty, _) = self.type_name()?;
            let (name, loc) = self.name("the name of a parameter")?;
            params.push(Param { ty, name, loc });
            if !self.eat(",") {
                break;
            }
        }
        self.expect(")")?;
        let body = if self.eat(";") {
            None
        } else {
            if !self.at("{") {
                return Err((
                    self.loc(),
                    format!("expected `{{` or `;` but found {}", self.found()),
                ));
            }
            Some(self.block()?)
        };
        // The token before the cursor is the closing brace.
        let end = self.toks[self.pos.saturating_sub(1)].loc;
        Ok(Function {
            ret,
            name,
            params,
            body,
            loc,
            end,
        })
    }

    /// `name`, `name[n]`, `name = value`, … up to and including the `;`.
    fn declarators(&mut self, ty: TypeName, is_const: bool) -> Result<Vec<Decl>> {
        let mut decls = Vec::new();
        loop {
            let (name, loc) = self.name("the name of a variable")?;
            let array = if self.eat("[") {
                let size = if self.at("]") {
                    None
                } else {
                    Some(self.expr()?)
                };
                self.expect("]")?;
                Some(size)
            } else {
                None
            };
            let init = if self.eat("=") {
                let at = self.loc();
                if self.eat("{") {
                    let mut items = Vec::new();
                    while !self.at("}") {
                        items.push(self.assignment()?);
                        if !self.eat(",") {
                            break;
                        }
                    }
                    self.expect("}")?;
                    Some(Init::List(items, at))
                } else {
                    Some(Init::Expr(self.assignment()?))
                }
            } else {
                None
            };
            decls.push(Decl {
                is_const,
                ty,
                name,
                array,
                init,
                loc,
            });
            if !self.eat(",") {
                break;
            }
        }
        self.expect(";")?;
        Ok(decls)
    }

    // ─────────────────────────── statements ───────────────────────────

    fn block(&mut self) -> Result<Vec<Stmt>> {
        let open = self.loc();
        self.expect("{")?;
        let mut stmts = Vec::new();
        while !self.at("}") {
            if *self.peek() == Tok::Eof {
                return Err((open, "this `{` is never closed".into()));
            }
            stmts.push(self.stmt()?);
        }
        self.advance();
        Ok(stmts)
    }

    fn condition(&mut self) -> Result<Expr> {
        self.expect("(")?;
        let e = self.expr()?;
        self.expect(")")?;
        Ok(e)
    }

    fn stmt(&mut self) -> Result<Stmt> {
        let loc = self.loc();
        if let Tok::Asm(text) = self.peek() {
            self.advance();
            return Ok(Stmt::Asm(text.clone(), loc));
        }
        if self.at("{") {
            return Ok(Stmt::Block(self.block()?));
        }
        if self.eat(";") {
            return Ok(Stmt::Empty);
        }
        if self.at_type() {
            let (ty, is_const) = self.type_name()?;
            return Ok(Stmt::Locals(self.declarators(ty, is_const)?));
        }
        if self.eat_word("if") {
            let condition = self.condition()?;
            let then = Box::new(self.stmt()?);
            let otherwise = if self.eat_word("else") {
                Some(Box::new(self.stmt()?))
            } else {
                None
            };
            return Ok(Stmt::If(condition, then, otherwise));
        }
        if self.eat_word("while") {
            let condition = self.condition()?;
            return Ok(Stmt::While(condition, Box::new(self.stmt()?)));
        }
        if self.eat_word("do") {
            let body = Box::new(self.stmt()?);
            if !self.eat_word("while") {
                return Err((
                    self.loc(),
                    format!("expected `while` but found {}", self.found()),
                ));
            }
            let condition = self.condition()?;
            self.expect(";")?;
            return Ok(Stmt::DoWhile(body, condition));
        }
        if self.eat_word("for") {
            self.expect("(")?;
            let init = if self.eat(";") {
                None
            } else if self.at_type() {
                let (ty, is_const) = self.type_name()?;
                Some(Box::new(Stmt::Locals(self.declarators(ty, is_const)?)))
            } else {
                let e = self.expr()?;
                self.expect(";")?;
                Some(Box::new(Stmt::Expr(e)))
            };
            let condition = if self.at(";") {
                None
            } else {
                Some(self.expr()?)
            };
            self.expect(";")?;
            let step = if self.at(")") {
                None
            } else {
                Some(self.expr()?)
            };
            self.expect(")")?;
            return Ok(Stmt::For(init, condition, step, Box::new(self.stmt()?)));
        }
        if self.eat_word("return") {
            let value = if self.at(";") {
                None
            } else {
                Some(self.expr()?)
            };
            self.expect(";")?;
            return Ok(Stmt::Return(value, loc));
        }
        if self.eat_word("break") {
            self.expect(";")?;
            return Ok(Stmt::Break(loc));
        }
        if self.eat_word("continue") {
            self.expect(";")?;
            return Ok(Stmt::Continue(loc));
        }
        let e = self.expr()?;
        self.expect(";")?;
        Ok(Stmt::Expr(e))
    }

    // ────────────────────────── expressions ──────────────────────────

    pub fn expr(&mut self) -> Result<Expr> {
        self.assignment()
    }

    fn assignment(&mut self) -> Result<Expr> {
        let left = self.conditional()?;
        let loc = self.loc();
        let op = match self.peek() {
            Tok::Punct("=") => None,
            Tok::Punct("+=") => Some(BinOp::Add),
            Tok::Punct("-=") => Some(BinOp::Sub),
            Tok::Punct("*=") => Some(BinOp::Mul),
            Tok::Punct("/=") => Some(BinOp::Div),
            Tok::Punct("%=") => Some(BinOp::Mod),
            Tok::Punct("&=") => Some(BinOp::And),
            Tok::Punct("|=") => Some(BinOp::Or),
            Tok::Punct("^=") => Some(BinOp::Xor),
            Tok::Punct("<<=") => Some(BinOp::Shl),
            Tok::Punct(">>=") => Some(BinOp::Shr),
            _ => return Ok(left),
        };
        self.advance();
        // Right to left: a = b = c.
        let right = self.assignment()?;
        Ok(Expr {
            kind: ExprKind::Assign(op, Box::new(left), Box::new(right)),
            loc,
        })
    }

    fn conditional(&mut self) -> Result<Expr> {
        let condition = self.binary(0)?;
        let loc = self.loc();
        if !self.eat("?") {
            return Ok(condition);
        }
        let then = self.expr()?;
        self.expect(":")?;
        let otherwise = self.conditional()?;
        Ok(Expr {
            kind: ExprKind::Cond(Box::new(condition), Box::new(then), Box::new(otherwise)),
            loc,
        })
    }

    /// Binary operators, weakest first, as in C.
    fn binary_op(&self) -> Option<(BinOp, u8)> {
        let Tok::Punct(p) = self.peek() else {
            return None;
        };
        Some(match *p {
            "||" => (BinOp::LogOr, 0),
            "&&" => (BinOp::LogAnd, 1),
            "|" => (BinOp::Or, 2),
            "^" => (BinOp::Xor, 3),
            "&" => (BinOp::And, 4),
            "==" => (BinOp::Eq, 5),
            "!=" => (BinOp::Ne, 5),
            "<" => (BinOp::Lt, 6),
            "<=" => (BinOp::Le, 6),
            ">" => (BinOp::Gt, 6),
            ">=" => (BinOp::Ge, 6),
            "<<" => (BinOp::Shl, 7),
            ">>" => (BinOp::Shr, 7),
            "+" => (BinOp::Add, 8),
            "-" => (BinOp::Sub, 8),
            "*" => (BinOp::Mul, 9),
            "/" => (BinOp::Div, 9),
            "%" => (BinOp::Mod, 9),
            _ => return None,
        })
    }

    fn binary(&mut self, min_level: u8) -> Result<Expr> {
        let mut left = self.unary()?;
        while let Some((op, level)) = self.binary_op() {
            if level < min_level {
                break;
            }
            let loc = self.loc();
            self.advance();
            let right = self.binary(level + 1)?;
            left = Expr {
                kind: ExprKind::Binary(op, Box::new(left), Box::new(right)),
                loc,
            };
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr> {
        let loc = self.loc();
        let op = match self.peek() {
            Tok::Punct("-") => Some(UnOp::Neg),
            Tok::Punct("~") => Some(UnOp::BitNot),
            Tok::Punct("!") => Some(UnOp::Not),
            Tok::Punct("*") => Some(UnOp::Deref),
            Tok::Punct("&") => Some(UnOp::Addr),
            Tok::Punct("++") => Some(UnOp::PreInc),
            Tok::Punct("--") => Some(UnOp::PreDec),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let operand = Box::new(self.unary()?);
            return Ok(Expr {
                kind: ExprKind::Unary(op, operand),
                loc,
            });
        }
        if self.eat("+") {
            return self.unary();
        }
        if self.eat_word("sizeof") {
            // `sizeof(type)`, or `sizeof` of an expression.
            if self.at("(") && self.type_follows(1) {
                self.advance();
                let (ty, _) = self.type_name()?;
                self.expect(")")?;
                return Ok(Expr {
                    kind: ExprKind::SizeofType(ty),
                    loc,
                });
            }
            let operand = Box::new(self.unary()?);
            return Ok(Expr {
                kind: ExprKind::SizeofExpr(operand),
                loc,
            });
        }
        // A cast: `(type) value`.
        if self.at("(") && self.type_follows(1) {
            self.advance();
            let (ty, _) = self.type_name()?;
            self.expect(")")?;
            let operand = Box::new(self.unary()?);
            return Ok(Expr {
                kind: ExprKind::Cast(ty, operand),
                loc,
            });
        }
        self.postfix()
    }

    fn type_follows(&self, ahead: usize) -> bool {
        matches!(self.peek_at(ahead), Tok::Ident(w)
            if ["u8", "i8", "u16", "i16", "void", "const"].contains(&w.as_str()))
    }

    fn postfix(&mut self) -> Result<Expr> {
        let mut e = self.primary()?;
        loop {
            let loc = self.loc();
            if self.eat("[") {
                let index = self.expr()?;
                self.expect("]")?;
                e = Expr {
                    kind: ExprKind::Index(Box::new(e), Box::new(index)),
                    loc,
                };
            } else if self.eat("++") {
                e = Expr {
                    kind: ExprKind::Unary(UnOp::PostInc, Box::new(e)),
                    loc,
                };
            } else if self.eat("--") {
                e = Expr {
                    kind: ExprKind::Unary(UnOp::PostDec, Box::new(e)),
                    loc,
                };
            } else {
                return Ok(e);
            }
        }
    }

    fn primary(&mut self) -> Result<Expr> {
        let loc = self.loc();
        let kind = match self.peek() {
            Tok::Num(n) => {
                self.advance();
                ExprKind::Num(*n)
            }
            Tok::Str(bytes) => {
                self.advance();
                // "Adjacent" "strings" are one string.
                let mut all = bytes.clone();
                while let Tok::Str(more) = self.peek() {
                    all.extend_from_slice(more);
                    self.advance();
                }
                ExprKind::Str(all)
            }
            Tok::Punct("(") => {
                self.advance();
                let inner = self.expr()?;
                self.expect(")")?;
                return Ok(inner);
            }
            Tok::Ident(_) => {
                let (name, _) = self.name("a value")?;
                if self.eat("(") {
                    let mut args = Vec::new();
                    while !self.at(")") {
                        args.push(self.assignment()?);
                        if !self.eat(",") {
                            break;
                        }
                    }
                    self.expect(")")?;
                    ExprKind::Call(name, args)
                } else {
                    ExprKind::Ident(name)
                }
            }
            _ => return Err((loc, format!("expected a value but found {}", self.found()))),
        };
        Ok(Expr { kind, loc })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::Lexer;

    fn parse(source: &str) -> Result<Vec<Item>> {
        let files = [("a.c", source)];
        let (tokens, _) = Lexer::new(&files).run("a.c").expect("the source lexes");
        Parser::new(&tokens).program()
    }

    /// An expression, printed back fully parenthesised.
    fn shape(source: &str) -> String {
        fn show(e: &Expr) -> String {
            match &e.kind {
                ExprKind::Num(n) => n.to_string(),
                ExprKind::Str(s) => format!("{:?}", String::from_utf8_lossy(s)),
                ExprKind::Ident(name) => name.clone(),
                ExprKind::Unary(op, a) => format!("({op:?} {})", show(a)),
                ExprKind::Binary(op, a, b) => format!("({op:?} {} {})", show(a), show(b)),
                ExprKind::Assign(None, a, b) => format!("(= {} {})", show(a), show(b)),
                ExprKind::Assign(Some(op), a, b) => format!("({op:?}= {} {})", show(a), show(b)),
                ExprKind::Cond(c, a, b) => format!("(? {} {} {})", show(c), show(a), show(b)),
                ExprKind::Call(f, args) => {
                    let args: Vec<String> = args.iter().map(show).collect();
                    format!("{f}({})", args.join(", "))
                }
                ExprKind::Index(a, i) => format!("{}[{}]", show(a), show(i)),
                ExprKind::Cast(ty, a) => format!(
                    "({:?}{} {})",
                    ty.base,
                    "*".repeat(ty.pointers as usize),
                    show(a)
                ),
                ExprKind::SizeofType(ty) => format!("sizeof({:?})", ty.base),
                ExprKind::SizeofExpr(a) => format!("sizeof {}", show(a)),
            }
        }
        let items = parse(&format!("void f() {{ {source}; }}")).unwrap();
        let Item::Function(f) = &items[0] else {
            panic!("not a function");
        };
        match &f.body.as_ref().unwrap()[0] {
            Stmt::Expr(e) => show(e),
            other => panic!("not an expression: {other:?}"),
        }
    }

    #[test]
    fn precedence_and_associativity_are_c_s() {
        assert_eq!(shape("1 + 2 * 3"), "(Add 1 (Mul 2 3))");
        assert_eq!(shape("a - b - c"), "(Sub (Sub a b) c)");
        assert_eq!(shape("a = b = c"), "(= a (= b c))");
        assert_eq!(shape("a << 1 | b & 3"), "(Or (Shl a 1) (And b 3))");
        assert_eq!(
            shape("a == b && c < d || e"),
            "(LogOr (LogAnd (Eq a b) (Lt c d)) e)"
        );
        assert_eq!(shape("a ? b : c ? d : e"), "(? a b (? c d e))");
        assert_eq!(shape("x += y * 2"), "(Add= x (Mul y 2))");
        assert_eq!(
            shape("-a + ~b * !c"),
            "(Add (Neg a) (Mul (BitNot b) (Not c)))"
        );
        assert_eq!(
            shape("*p++ = a[i + 1]"),
            "(= (Deref (PostInc p)) a[(Add i 1)])"
        );
        assert_eq!(shape("&a[2]"), "(Addr a[2])");
        assert_eq!(shape("--x - -y"), "(Sub (PreDec x) (Neg y))");
        assert_eq!(shape("(u16)a * b"), "(Mul (U16 a) b)");
        assert_eq!(shape("*(u8*)0x8801 = 1"), "(= (Deref (U8* 34817)) 1)");
        assert_eq!(shape("f(a, g(), 3)[0]"), "f(a, g(), 3)[0]");
        assert_eq!(
            shape("sizeof(u16) + sizeof table"),
            "(Add sizeof(U16) sizeof table)"
        );
        assert_eq!(shape("(a) + 1"), "(Add a 1)");
        assert_eq!(shape("\"ab\" \"cd\""), "\"abcd\"");
    }

    #[test]
    fn declarations() {
        let items = parse(
            "const u8 N = 4;\n u8* a, p, grid[N * 2];\n const u8 text[] = \"hi\";\n \
             u16 big[] = { 1, 2, 3, };\n void f(void);\n u8 g(u8 a, const u8 *s) { return a; }\n \
             asm { .include \"x.asm\" }",
        )
        .unwrap();
        assert_eq!(items.len(), 9);
        let Item::Global(n) = &items[0] else { panic!() };
        assert!(n.is_const && n.array.is_none() && n.init.is_some());
        let Item::Global(p) = &items[2] else { panic!() };
        assert_eq!(
            (p.name.as_str(), p.ty.pointers),
            ("p", 1),
            "the `*` belongs to the type"
        );
        let Item::Global(grid) = &items[3] else {
            panic!()
        };
        assert!(matches!(grid.array, Some(Some(_))));
        let Item::Global(text) = &items[4] else {
            panic!()
        };
        assert!(matches!(text.array, Some(None)));
        let Item::Global(big) = &items[5] else {
            panic!()
        };
        assert!(matches!(&big.init, Some(Init::List(v, _)) if v.len() == 3));
        let Item::Function(f) = &items[6] else {
            panic!()
        };
        assert!(f.body.is_none() && f.params.is_empty());
        let Item::Function(g) = &items[7] else {
            panic!()
        };
        assert_eq!(g.params.len(), 2);
        assert_eq!(
            g.params[1].ty,
            TypeName {
                base: Base::U8,
                pointers: 1
            }
        );
        assert!(matches!(&items[8], Item::Asm(text, _) if text.contains(".include")));
    }

    #[test]
    fn statements() {
        let items = parse(
            "void f() {
                u8 i = 0, j;
                if (i) j = 1; else { j = 2; }
                while (i < 3) i++;
                do { i--; } while (i);
                for (u8 k = 0; k < 4; k++) { if (k == 2) continue; break; }
                for (;;) ;
                asm { nop }
                return;
            }",
        )
        .unwrap();
        let Item::Function(f) = &items[0] else {
            panic!()
        };
        let body = f.body.as_ref().unwrap();
        assert_eq!(body.len(), 8);
        assert!(matches!(&body[0], Stmt::Locals(d) if d.len() == 2));
        assert!(matches!(&body[1], Stmt::If(_, _, Some(_))));
        assert!(matches!(&body[4], Stmt::For(Some(_), Some(_), Some(_), _)));
        assert!(matches!(&body[5], Stmt::For(None, None, None, _)));
        assert!(matches!(&body[6], Stmt::Asm(..)));
        assert_eq!(f.end.line, 10, "where the function ends");
    }

    #[test]
    fn errors_say_what_was_expected() {
        let err = |source: &str| {
            let (loc, message) = parse(source).unwrap_err();
            (loc.line, loc.col, message)
        };
        assert_eq!(
            err("u8 x = 1"),
            (1, 8, "expected `;` but found the end of the file".into())
        );
        assert_eq!(
            err("void f() { x = ; }"),
            (1, 16, "expected a value but found `;`".into())
        );
        assert_eq!(
            err("void f() { if x }"),
            (1, 15, "expected `(` but found `x`".into())
        );
        assert_eq!(
            err("u8 while;"),
            (
                1,
                4,
                "`while` is a keyword and cannot be the name of a variable".into()
            )
        );
        assert_eq!(
            err("x = 1;").2,
            "expected a type (`u8`, `i8`, `u16`, `i16`, `void`) but found `x`"
        );
        assert_eq!(
            err("void f() {\n x = 1;\n"),
            (1, 10, "this `{` is never closed".into())
        );
        assert_eq!(
            err("void f() u8"),
            (1, 10, "expected `{` or `;` but found `u8`".into())
        );
        assert_eq!(err("void f() { a[1; }").2, "expected `]` but found `;`");
        assert_eq!(
            err("void f() { do x++; until (x); }").2,
            "expected `while` but found `until`"
        );
    }
}
