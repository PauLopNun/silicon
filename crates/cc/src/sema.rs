//! What the program means: names are resolved, types are checked, constants
//! are computed and every conversion is made explicit. The result is the
//! tree of `crate::ir`.
//!
//! The rules of the language are few (`docs/LANGUAGE.md`):
//!
//! * arithmetic happens in the wider of its two operand types, and is
//!   unsigned if the two differ only in sign;
//! * a constant takes the type of what it is combined with, when it fits;
//! * assigning converts silently between integer types, and nothing else.

use crate::ast::{self, Base, BinOp, ExprKind, Init, TypeName, UnOp};
use crate::ir::{Data, Datum, Expr, Function, Item, Kind, Op, Place, Program, Rel, Stmt, Ty, Var};
use crate::lex::Loc;
use silicon_asm::{Diagnostic, Pos};
use std::collections::HashMap;

/// Stop reporting after this many errors.
const MAX_ERRORS: usize = 50;

#[derive(Clone)]
enum Global {
    /// `const u8 N = 4;`: a name for a value, with no storage.
    Const(Expr),
    Var(Ty),
    Function {
        ret: Ty,
        params: Vec<Ty>,
        defined: bool,
        loc: Loc,
    },
}

/// A name declared inside a function.
enum Local {
    /// A variable or a parameter: a place in the function's frame.
    Var(Var, Ty),
    /// `const u8 N = 4;`: a name for a value, with no storage.
    Const(Expr),
}

/// How a constant is fitted to the type of the other operand.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Its value must be one the type can hold.
    Value,
    /// Its bits must fit: `x & ~1` is fine for an unsigned `x`.
    Bits,
    /// Like `Value`, and pointers may be compared with each other and
    /// with 0.
    Compare,
}

pub(crate) struct Sema<'a> {
    files: &'a [String],
    errors: Vec<Diagnostic>,
    globals: HashMap<String, Global>,
    /// The variables in sight, innermost block last.
    scopes: Vec<HashMap<String, Local>>,
    /// Bytes of the frame in use at this point, and the most ever in use.
    frame: u32,
    frame_max: u32,
    /// Return type of the function being read.
    ret: Ty,
    /// How many loops enclose this point.
    loops: u32,
    program: Program,
}

fn range(ty: &Ty) -> std::ops::RangeInclusive<i64> {
    match ty {
        Ty::U8 => 0..=255,
        Ty::I8 => -128..=127,
        Ty::I16 => -32768..=32767,
        _ => 0..=65535,
    }
}

/// The values whose bits fit in the width of `ty`, read either way.
fn bits(ty: &Ty) -> std::ops::RangeInclusive<i64> {
    if ty.wide() {
        -32768..=65535
    } else {
        -128..=255
    }
}

/// `"a u8"`, `"an i16"`.
fn a(ty: &Ty) -> String {
    let name = ty.name();
    let article = if name.starts_with('i') { "an" } else { "a" };
    format!("{article} `{name}`")
}

/// The result of an operation on two constants, or `None` for a division
/// by zero. This must agree with what the generated code computes.
fn fold(op: Op, x: i64, y: i64, ty: &Ty) -> Option<i64> {
    let v = match op {
        Op::Add => x.wrapping_add(y),
        Op::Sub => x.wrapping_sub(y),
        Op::Mul => x.wrapping_mul(y),
        Op::DivU | Op::DivS if y == 0 => return None,
        Op::ModU | Op::ModS if y == 0 => return None,
        Op::DivU | Op::DivS => x.wrapping_div(y),
        Op::ModU | Op::ModS => x.wrapping_rem(y),
        Op::And => x & y,
        Op::Or => x | y,
        Op::Xor => x ^ y,
        Op::Shl if (0..32).contains(&y) => x << y,
        Op::ShrU if (0..32).contains(&y) => x >> y,
        Op::Shl | Op::ShrU => 0,
        Op::ShrS => x >> y.clamp(0, 63),
    };
    Some(ty.wrap(v))
}

/// Can the statements finish and let whatever follows them run? Judged by
/// their shape alone: conditions are not worked out, except those that are
/// plainly constants (`while (1)`).
fn runs_off_the_end(stmts: &[Stmt]) -> bool {
    !stmts.iter().any(never_passes)
}

/// Does control never go from this statement on to the next?
fn never_passes(s: &Stmt) -> bool {
    let always = |e: &Option<Expr>| match e {
        None => true,
        Some(e) => e.constant().is_some_and(|v| v != 0),
    };
    match s {
        Stmt::Return(..) | Stmt::Break(_) | Stmt::Continue(_) => true,
        // Assembly is trusted: a function may end in a block that leaves
        // the result in `a` or returns by itself.
        Stmt::Asm(..) => true,
        Stmt::Block(stmts) => !runs_off_the_end(stmts),
        Stmt::If(_, then, otherwise) => !runs_off_the_end(then) && !runs_off_the_end(otherwise),
        // A loop without an end is left only by `return` or `break`.
        Stmt::Loop {
            before,
            body,
            after,
            ..
        } => always(before) && (after.is_none() || always(after)) && !breaks(body),
        Stmt::Expr(_) => false,
    }
}

/// Is there a `break` that leaves the loop these statements are the body
/// of? (Those of loops inside it leave only those.)
fn breaks(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::Break(_) => true,
        Stmt::Block(stmts) => breaks(stmts),
        Stmt::If(_, then, otherwise) => breaks(then) || breaks(otherwise),
        _ => false,
    })
}

impl<'a> Sema<'a> {
    /// Analyse a whole program.
    pub fn run(items: &[ast::Item], files: &'a [String]) -> Result<Program, Vec<Diagnostic>> {
        let mut sema = Sema {
            files,
            errors: Vec::new(),
            globals: HashMap::new(),
            scopes: Vec::new(),
            frame: 0,
            frame_max: 0,
            ret: Ty::Void,
            loops: 0,
            program: Program::default(),
        };
        // First everything that is declared, so that functions can be used
        // before the place they are written.
        for item in items {
            match item {
                ast::Item::Global(decl) => sema.global(decl),
                ast::Item::Function(f) => sema.signature(f),
                ast::Item::Asm(..) => {}
            }
        }
        match sema.globals.get("main").cloned() {
            Some(Global::Function { params, loc, .. }) if !params.is_empty() => {
                sema.error(
                    loc,
                    "`main` takes no arguments: nobody is there to give them",
                );
            }
            Some(Global::Function { defined: true, .. }) => {}
            _ => {
                let start = Loc {
                    file: 0,
                    line: 1,
                    col: 1,
                };
                sema.error(start, "there is no `main` function; a program starts there");
            }
        }
        for item in items {
            match item {
                ast::Item::Function(f) => {
                    if let Some(function) = sema.function(f) {
                        sema.program.items.push(Item::Function(function));
                    }
                }
                ast::Item::Asm(text, loc) => sema.program.items.push(Item::Asm(text.clone(), *loc)),
                ast::Item::Global(_) => {}
            }
        }
        if sema.errors.is_empty() {
            Ok(sema.program)
        } else {
            Err(sema.errors)
        }
    }

    fn error(&mut self, loc: Loc, message: impl Into<String>) {
        if self.errors.len() >= MAX_ERRORS {
            return;
        }
        self.errors.push(Diagnostic {
            file: self.files[loc.file].clone(),
            pos: Pos {
                line: loc.line,
                col: loc.col,
            },
            message: message.into(),
        });
    }

    /// Report an error and give back an expression that causes no more.
    fn wrong(&mut self, loc: Loc, message: impl Into<String>) -> Expr {
        self.error(loc, message);
        Expr {
            kind: Kind::Const(0),
            ty: Ty::Error,
            loc,
        }
    }

    fn ty(&mut self, name: TypeName, loc: Loc) -> Ty {
        let mut ty = match name.base {
            Base::U8 => Ty::U8,
            Base::I8 => Ty::I8,
            Base::U16 => Ty::U16,
            Base::I16 => Ty::I16,
            Base::Void => Ty::Void,
        };
        if ty == Ty::Void && name.pointers > 0 {
            self.error(loc, "there are no pointers to `void`; use `u8*`");
            return Ty::Error;
        }
        for _ in 0..name.pointers {
            ty = Ty::Ptr(Box::new(ty));
        }
        ty
    }

    fn where_is(&self, loc: Loc) -> String {
        format!("{}:{}", self.files[loc.file], loc.line)
    }

    // ───────────────────────── declarations ─────────────────────────

    /// An expression that must have a value before the program runs.
    fn constant(&mut self, e: &ast::Expr, ty: &Ty) -> Expr {
        let value = self.rvalue(e, Some(ty));
        let value = self.convert(value, ty, false, e.loc);
        if value.ty != Ty::Error && !matches!(value.kind, Kind::Const(_) | Kind::Sym(..)) {
            return self.wrong(
                e.loc,
                "this must be a constant: it is needed before the program runs",
            );
        }
        value
    }

    fn datum(value: &Expr) -> Datum {
        match &value.kind {
            Kind::Sym(symbol, offset) => Datum::Addr(symbol.clone(), *offset),
            Kind::Const(v) if value.ty.wide() => Datum::Word(*v),
            Kind::Const(v) => Datum::Byte(*v),
            _ => Datum::Byte(0),
        }
    }

    /// The number of elements written between `[` and `]`.
    fn array_len(&mut self, e: &ast::Expr) -> Option<u32> {
        let size = self.rvalue(e, None);
        match size.constant() {
            _ if size.ty == Ty::Error => None,
            Some(n) if (1..=0x8000).contains(&n) => Some(n as u32),
            Some(n) => {
                self.error(e.loc, format!("an array cannot have {n} elements"));
                None
            }
            None => {
                self.error(e.loc, "the size of an array must be a constant");
                None
            }
        }
    }

    fn global(&mut self, decl: &ast::Decl) {
        let elem = self.ty(decl.ty, decl.loc);
        if elem == Ty::Void {
            self.error(decl.loc, "a variable cannot be `void`");
            return;
        }
        if let Some(earlier) = self.globals.get(&decl.name) {
            let what = match earlier {
                Global::Function { .. } => "a function",
                _ => "already declared",
            };
            self.error(decl.loc, format!("`{}` is {what}", decl.name));
            return;
        }
        let symbol = format!("_{}", decl.name);
        let Some(size) = &decl.array else {
            // One value.
            let init = match &decl.init {
                Some(Init::Expr(e)) => Some(self.constant(e, &elem)),
                Some(Init::List(_, loc)) => {
                    self.error(*loc, "only an array can be given a list of values");
                    None
                }
                None => None,
            };
            if decl.is_const {
                let Some(value) = init else {
                    self.error(
                        decl.loc,
                        format!("the constant `{}` needs a value", decl.name),
                    );
                    return;
                };
                self.globals.insert(decl.name.clone(), Global::Const(value));
            } else {
                self.program.data.push(Data {
                    symbol,
                    size: elem.size(),
                    rom: false,
                    init: init.map(|value| vec![Self::datum(&value)]),
                });
                self.globals.insert(decl.name.clone(), Global::Var(elem));
            }
            return;
        };

        // An array.
        let mut text_len = None;
        let values: Option<Vec<Expr>> = match &decl.init {
            Some(Init::List(items, _)) => {
                Some(items.iter().map(|e| self.constant(e, &elem)).collect())
            }
            Some(Init::Expr(e)) => match &e.kind {
                ExprKind::Str(bytes) if matches!(elem, Ty::U8 | Ty::I8) => {
                    text_len = Some(bytes.len());
                    let byte = |b: u8| Expr {
                        kind: Kind::Const(elem.wrap(b as i64)),
                        ty: elem.clone(),
                        loc: e.loc,
                    };
                    // The text and the zero that ends it.
                    Some(bytes.iter().copied().chain([0]).map(byte).collect())
                }
                ExprKind::Str(_) => {
                    self.error(e.loc, "a string can only fill an array of `u8`");
                    None
                }
                _ => {
                    self.error(
                        e.loc,
                        "an array is given its values as a list: `{ 1, 2, 3 }`",
                    );
                    None
                }
            },
            None => None,
        };
        let len = match (size, &values) {
            (Some(e), _) => self.array_len(e),
            (None, Some(values)) => Some(values.len() as u32),
            (None, None) => {
                self.error(
                    decl.loc,
                    format!("`{}` has no size and no values to count", decl.name),
                );
                None
            }
        };
        let Some(len) = len else {
            return;
        };
        let mut init = values.map(|v| v.iter().map(Self::datum).collect::<Vec<Datum>>());
        if let Some(init) = &mut init {
            // A string may fill its array exactly, without the final zero.
            if text_len == Some(len as usize) {
                init.pop();
            }
            if init.len() > len as usize {
                let given = text_len.map_or(init.len(), |t| t + 1);
                self.error(
                    decl.loc,
                    format!(
                        "`{}` has room for {len} values and {given} are given",
                        decl.name
                    ),
                );
                return;
            }
            let zero = if elem.wide() {
                Datum::Word(0)
            } else {
                Datum::Byte(0)
            };
            init.resize(len as usize, zero);
        } else if decl.is_const {
            self.error(
                decl.loc,
                format!("the constant `{}` needs its values", decl.name),
            );
            return;
        }
        self.program.data.push(Data {
            symbol,
            size: len * elem.size(),
            rom: decl.is_const,
            init,
        });
        let ty = Ty::Array(Box::new(elem), len);
        self.globals.insert(decl.name.clone(), Global::Var(ty));
    }

    fn signature(&mut self, f: &ast::Function) {
        let ret = self.ty(f.ret, f.loc);
        let mut params = Vec::new();
        for p in &f.params {
            let ty = self.ty(p.ty, p.loc);
            if ty == Ty::Void {
                self.error(p.loc, "a parameter cannot be `void`");
            }
            params.push(ty);
        }
        let defined = f.body.is_some();
        match self.globals.get(&f.name).cloned() {
            None => {}
            Some(Global::Function {
                ret: r,
                params: p,
                defined: was,
                loc,
            }) => {
                if r != ret || p != params {
                    let at = self.where_is(loc);
                    self.error(
                        f.loc,
                        format!("`{}` is declared differently at {at}", f.name),
                    );
                    return;
                }
                if was && defined {
                    let at = self.where_is(loc);
                    self.error(f.loc, format!("`{}` is already defined at {at}", f.name));
                    return;
                }
                if was {
                    return;
                }
            }
            Some(_) => {
                self.error(
                    f.loc,
                    format!("`{}` is already declared as a variable", f.name),
                );
                return;
            }
        }
        let function = Global::Function {
            ret,
            params,
            defined,
            loc: f.loc,
        };
        self.globals.insert(f.name.clone(), function);
    }

    fn function(&mut self, f: &ast::Function) -> Option<Function> {
        let body = f.body.as_ref()?;
        let Some(Global::Function { ret, params, .. }) = self.globals.get(&f.name).cloned() else {
            return None;
        };
        self.scopes = vec![HashMap::new()];
        (self.frame, self.frame_max, self.loops) = (0, 0, 0);
        self.ret = ret;
        // Arguments are pushed in the order they are written, two bytes
        // each, so the first one is the farthest from the return address.
        let count = params.len() as u32;
        for (i, (param, ty)) in f.params.iter().zip(params).enumerate() {
            let var = Var::Param(2 * (count - 1 - i as u32));
            if self.scopes[0]
                .insert(param.name.clone(), Local::Var(var, ty))
                .is_some()
            {
                self.error(
                    param.loc,
                    format!("two parameters are called `{}`", param.name),
                );
            }
        }
        let body = self.block(body);
        self.scopes.clear();
        if self.ret != Ty::Void && runs_off_the_end(&body) {
            let message = format!(
                "`{}` must return {}, and its end can be reached without a `return`",
                f.name,
                a(&self.ret)
            );
            self.error(f.end, message);
        }
        Some(Function {
            name: format!("_{}", f.name),
            frame: self.frame_max,
            body,
            loc: f.loc,
            end: f.end,
        })
    }

    // ─────────────────────────── statements ───────────────────────────

    /// Statements in a scope of their own. Their variables give their room
    /// back at the end, for the next block to use.
    fn block(&mut self, stmts: &[ast::Stmt]) -> Vec<Stmt> {
        let frame = self.frame;
        self.scopes.push(HashMap::new());
        let mut out = Vec::new();
        for s in stmts {
            self.stmt(s, &mut out);
        }
        self.scopes.pop();
        self.frame = frame;
        out
    }

    fn body(&mut self, s: &ast::Stmt) -> Vec<Stmt> {
        self.block(std::slice::from_ref(s))
    }

    fn loop_body(&mut self, s: &ast::Stmt) -> Vec<Stmt> {
        self.loops += 1;
        let body = self.body(s);
        self.loops -= 1;
        body
    }

    fn stmt(&mut self, s: &ast::Stmt, out: &mut Vec<Stmt>) {
        match s {
            ast::Stmt::Empty => {}
            ast::Stmt::Block(stmts) => out.push(Stmt::Block(self.block(stmts))),
            ast::Stmt::Asm(text, loc) => out.push(Stmt::Asm(text.clone(), *loc)),
            ast::Stmt::Expr(e) => {
                let value = self.expr(e, None);
                out.push(Stmt::Expr(self.settle(value, None)));
            }
            ast::Stmt::If(condition, then, otherwise) => {
                let condition = self.truth(condition);
                let then = self.body(then);
                let otherwise = otherwise.as_ref().map_or(Vec::new(), |s| self.body(s));
                out.push(Stmt::If(condition, then, otherwise));
            }
            ast::Stmt::While(condition, body) => {
                let before = Some(self.truth(condition));
                let body = self.loop_body(body);
                out.push(Stmt::Loop {
                    before,
                    body,
                    step: None,
                    after: None,
                });
            }
            ast::Stmt::DoWhile(body, condition) => {
                let body = self.loop_body(body);
                let after = Some(self.truth(condition));
                out.push(Stmt::Loop {
                    before: None,
                    body,
                    step: None,
                    after,
                });
            }
            ast::Stmt::For(init, condition, step, body) => {
                // The variable of a `for` belongs to the loop.
                let frame = self.frame;
                self.scopes.push(HashMap::new());
                let mut stmts = Vec::new();
                if let Some(init) = init {
                    self.stmt(init, &mut stmts);
                }
                let before = condition.as_ref().map(|c| self.truth(c));
                let step = step.as_ref().map(|e| {
                    let value = self.expr(e, None);
                    self.settle(value, None)
                });
                let body = self.loop_body(body);
                stmts.push(Stmt::Loop {
                    before,
                    body,
                    step,
                    after: None,
                });
                self.scopes.pop();
                self.frame = frame;
                out.push(Stmt::Block(stmts));
            }
            ast::Stmt::Return(value, loc) => {
                let ret = self.ret.clone();
                let value = match value {
                    Some(e) if ret == Ty::Void => {
                        self.error(e.loc, "this function returns nothing");
                        None
                    }
                    Some(e) => {
                        let value = self.rvalue(e, Some(&ret));
                        Some(self.convert(value, &ret, false, e.loc))
                    }
                    None if ret != Ty::Void => {
                        self.error(*loc, format!("this function must return {}", a(&ret)));
                        None
                    }
                    None => None,
                };
                out.push(Stmt::Return(value, *loc));
            }
            ast::Stmt::Break(loc) | ast::Stmt::Continue(loc) => {
                let is_break = matches!(s, ast::Stmt::Break(_));
                if self.loops == 0 {
                    let word = if is_break { "break" } else { "continue" };
                    self.error(*loc, format!("`{word}` is only for inside a loop"));
                } else if is_break {
                    out.push(Stmt::Break(*loc));
                } else {
                    out.push(Stmt::Continue(*loc));
                }
            }
            ast::Stmt::Locals(decls) => {
                for decl in decls {
                    self.local(decl, out);
                }
            }
        }
    }

    fn local(&mut self, decl: &ast::Decl, out: &mut Vec<Stmt>) {
        let elem = self.ty(decl.ty, decl.loc);
        if elem == Ty::Void {
            self.error(decl.loc, "a variable cannot be `void`");
            return;
        }
        let ty = match &decl.array {
            None => elem,
            Some(Some(size)) => match self.array_len(size) {
                Some(len) => Ty::Array(Box::new(elem), len),
                None => return,
            },
            Some(None) => {
                self.error(decl.loc, format!("the array `{}` needs a size", decl.name));
                return;
            }
        };
        if decl.is_const {
            self.local_constant(decl, &ty);
            return;
        }
        // The value is worked out before the name exists: `u8 x = x + 1`
        // means the `x` outside.
        let init = match &decl.init {
            Some(_) if !ty.scalar() => {
                self.error(
                    decl.loc,
                    "an array inside a function starts with no values; assign its elements, \
                     or make it a global",
                );
                None
            }
            Some(Init::List(_, loc)) => {
                self.error(*loc, "only an array can be given a list of values");
                None
            }
            Some(Init::Expr(e)) => {
                let value = self.rvalue(e, Some(&ty));
                Some(self.convert(value, &ty, false, e.loc))
            }
            None => None,
        };
        let var = Var::Local(self.frame);
        self.frame += ty.size();
        self.frame_max = self.frame_max.max(self.frame);
        self.declare(decl, Local::Var(var.clone(), ty.clone()));
        if let Some(value) = init {
            out.push(Stmt::Expr(Expr {
                kind: Kind::Store(Box::new(Place::Var(var)), Box::new(value)),
                ty,
                loc: decl.loc,
            }));
        }
    }

    /// `const u8 N = 4;` inside a function: a name for a value, as outside
    /// one, known from here to the end of the block.
    fn local_constant(&mut self, decl: &ast::Decl, ty: &Ty) {
        if !ty.scalar() {
            self.error(
                decl.loc,
                "a constant array is kept in ROM; declare it outside the functions",
            );
            return;
        }
        let value = match &decl.init {
            Some(Init::Expr(e)) => self.constant(e, ty),
            Some(Init::List(_, loc)) => {
                self.error(*loc, "only an array can be given a list of values");
                return;
            }
            None => {
                self.error(
                    decl.loc,
                    format!("the constant `{}` needs a value", decl.name),
                );
                return;
            }
        };
        self.declare(decl, Local::Const(value));
    }

    /// Make a name known in the innermost block.
    fn declare(&mut self, decl: &ast::Decl, local: Local) {
        let scope = self.scopes.last_mut().expect("inside a function");
        if scope.insert(decl.name.clone(), local).is_some() {
            self.error(
                decl.loc,
                format!("`{}` is already declared in this block", decl.name),
            );
        }
    }

    // ────────────────────────── conversions ──────────────────────────

    fn retype(e: Expr, to: &Ty) -> Expr {
        let kind = match e.kind {
            Kind::Const(v) => Kind::Const(to.wrap(v)),
            other => other,
        };
        Expr {
            kind,
            ty: to.clone(),
            loc: e.loc,
        }
    }

    /// 8 bits to 16, by the sign of what is being widened.
    fn widen(e: Expr, to: &Ty) -> Expr {
        if e.constant().is_some() {
            return Self::retype(e, to);
        }
        let loc = e.loc;
        Expr {
            kind: Kind::Widen(e.ty.signed(), Box::new(e)),
            ty: to.clone(),
            loc,
        }
    }

    fn narrow(e: Expr, to: &Ty) -> Expr {
        if e.constant().is_some() {
            return Self::retype(e, to);
        }
        let loc = e.loc;
        Expr {
            kind: Kind::Narrow(Box::new(e)),
            ty: to.clone(),
            loc,
        }
    }

    /// Give a constant that stands alone a type: the one wanted here if it
    /// fits, else the smallest that holds it.
    fn settle(&mut self, e: Expr, hint: Option<&Ty>) -> Expr {
        if e.ty != Ty::Lit {
            return e;
        }
        let v = e.constant().unwrap_or(0);
        let ty = match hint {
            Some(hint) if hint.is_int() && bits(hint).contains(&v) => hint.clone(),
            _ => match v {
                0..=255 => Ty::U8,
                -128..=-1 => Ty::I8,
                256..=65535 => Ty::U16,
                -32768..=-129 => Ty::I16,
                _ => return self.wrong(e.loc, format!("{v} does not fit in 16 bits")),
            },
        };
        Self::retype(e, &ty)
    }

    /// Make `e` a `to`, as assignment does (`explicit` is a cast, which may
    /// do more).
    fn convert(&mut self, e: Expr, to: &Ty, explicit: bool, loc: Loc) -> Expr {
        if e.ty == Ty::Error || *to == Ty::Error {
            return Expr { ty: Ty::Error, ..e };
        }
        if e.ty == *to {
            return e;
        }
        let v = e.constant().unwrap_or(0);
        match (&e.ty, to) {
            (Ty::Lit, to) if to.is_int() => {
                if !explicit && !bits(to).contains(&v) {
                    return self.wrong(loc, format!("{v} does not fit in {}", a(to)));
                }
                Self::retype(e, to)
            }
            (Ty::Lit, Ty::Ptr(_)) if v == 0 || (explicit && range(to).contains(&v)) => {
                Self::retype(e, to)
            }
            (from, to) if from.is_int() && to.is_int() => match (from.wide(), to.wide()) {
                (false, true) => Self::widen(e, to),
                (true, false) => Self::narrow(e, to),
                _ => Self::retype(e, to),
            },
            (Ty::Ptr(_), Ty::Ptr(_)) if explicit => Self::retype(e, to),
            (Ty::Ptr(_), to) if explicit && to.is_int() => {
                if to.wide() {
                    Self::retype(e, to)
                } else {
                    Self::narrow(e, to)
                }
            }
            (from, Ty::Ptr(_)) if explicit && from.is_int() => {
                if from.wide() {
                    Self::retype(e, to)
                } else {
                    Self::widen(e, to)
                }
            }
            (Ty::Void, _) => self.wrong(loc, "this has no value"),
            (from, to) => {
                let from = if *from == Ty::Lit {
                    "a number".to_string()
                } else {
                    a(from)
                };
                self.wrong(
                    loc,
                    format!(
                        "this is {from} and {} is needed; write `({})…` if that is meant",
                        a(to),
                        to.name()
                    ),
                )
            }
        }
    }

    /// Fit a constant to the typed operand it is combined with. Returns
    /// the two, the typed one possibly widened.
    fn with_constant(&mut self, typed: Expr, lit: Expr, mode: Mode) -> Option<(Expr, Expr)> {
        let v = lit.constant().unwrap_or(0);
        let ty = typed.ty.clone();
        if range(&ty).contains(&v) || (mode == Mode::Bits && bits(&ty).contains(&v)) {
            return Some((typed, Self::retype(lit, &ty)));
        }
        if ty.wide() {
            // There is nothing wider: the constant wraps, if its bits fit.
            if bits(&ty).contains(&v) {
                return Some((typed, Self::retype(lit, &ty)));
            }
            self.error(lit.loc, format!("{v} does not fit in 16 bits"));
            return None;
        }
        // Too large for 8 bits: do the operation in 16.
        let to = match (ty.signed(), v) {
            (true, -32768..=32767) | (false, -32768..=-1) => Ty::I16,
            (_, 0..=65535) => Ty::U16,
            _ => {
                self.error(lit.loc, format!("{v} does not fit in 16 bits"));
                return None;
            }
        };
        Some((Self::widen(typed, &to), Self::retype(lit, &to)))
    }

    /// Bring two operands to one type.
    fn unify(&mut self, x: Expr, y: Expr, loc: Loc, mode: Mode) -> Option<(Expr, Expr)> {
        if x.ty == Ty::Error || y.ty == Ty::Error {
            return None;
        }
        let (tx, ty) = (x.ty.clone(), y.ty.clone());
        match (&tx, &ty) {
            (Ty::Lit, Ty::Lit) => Some((x, y)),
            (Ty::Lit, t) if t.is_int() => self.with_constant(y, x, mode).map(|(y, x)| (x, y)),
            (t, Ty::Lit) if t.is_int() => self.with_constant(x, y, mode),
            (Ty::Lit, Ty::Ptr(_)) if mode == Mode::Compare && x.constant() == Some(0) => {
                Some((Self::retype(x, &ty), y))
            }
            (Ty::Ptr(_), Ty::Lit) if mode == Mode::Compare && y.constant() == Some(0) => {
                Some((x, Self::retype(y, &tx)))
            }
            (Ty::Ptr(p), Ty::Ptr(q)) if mode == Mode::Compare && p == q => Some((x, y)),
            (p, q) if p.is_int() && q.is_int() => {
                let to = match (p.wide(), q.wide()) {
                    _ if p == q => tx.clone(),
                    (false, false) => Ty::U8,
                    (true, true) => Ty::U16,
                    (true, false) => tx.clone(),
                    (false, true) => ty.clone(),
                };
                let fit = |e: Expr| {
                    if e.ty.wide() == to.wide() {
                        Self::retype(e, &to)
                    } else {
                        Self::widen(e, &to)
                    }
                };
                Some((fit(x), fit(y)))
            }
            (p, q) => {
                let name = |t: &Ty| {
                    if *t == Ty::Lit {
                        "a number".to_string()
                    } else {
                        a(t)
                    }
                };
                self.error(
                    loc,
                    format!("{} and {} cannot be combined here", name(p), name(q)),
                );
                None
            }
        }
    }

    // ────────────────────────── expressions ──────────────────────────

    fn lookup(&self, name: &str) -> Option<&Local> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }

    /// An expression whose value is used.
    fn rvalue(&mut self, e: &ast::Expr, hint: Option<&Ty>) -> Expr {
        let value = self.expr(e, hint);
        if value.ty == Ty::Void {
            return self.wrong(e.loc, "this returns nothing, and a value is needed here");
        }
        value
    }

    /// An expression used as a condition: a number or a pointer, true when
    /// it is not zero.
    fn truth(&mut self, e: &ast::Expr) -> Expr {
        let value = self.rvalue(e, None);
        self.settle(value, None)
    }

    fn load(place: Place, ty: Ty, loc: Loc) -> Expr {
        Expr {
            kind: Kind::Load(Box::new(place)),
            ty,
            loc,
        }
    }

    fn expr(&mut self, e: &ast::Expr, hint: Option<&Ty>) -> Expr {
        let loc = e.loc;
        match &e.kind {
            ExprKind::Num(n) => Expr {
                kind: Kind::Const(*n),
                ty: Ty::Lit,
                loc,
            },
            ExprKind::Str(bytes) => {
                self.program.strings.push(bytes.clone());
                Expr {
                    kind: Kind::Sym(format!("cc_str{}", self.program.strings.len() - 1), 0),
                    ty: Ty::Ptr(Box::new(Ty::U8)),
                    loc,
                }
            }
            ExprKind::Ident(_) | ExprKind::Index(..) | ExprKind::Unary(UnOp::Deref, _) => {
                self.variable(e)
            }
            ExprKind::Unary(op, operand) => self.unary(*op, operand, loc, hint),
            ExprKind::Binary(op, x, y) => self.binary(*op, x, y, loc, hint),
            ExprKind::Assign(op, target, value) => self.assign(*op, target, value, loc),
            ExprKind::Cond(condition, x, y) => {
                let condition = self.truth(condition);
                let x = self.rvalue(x, hint);
                let y = self.rvalue(y, hint);
                // Two constants are given a type each, then one between them.
                let both = x.ty == Ty::Lit && y.ty == Ty::Lit;
                let (x, y) = if both {
                    (self.settle(x, hint), self.settle(y, hint))
                } else {
                    (x, y)
                };
                let Some((x, y)) = self.unify(x, y, loc, Mode::Compare) else {
                    return self.silent(loc);
                };
                match condition.constant() {
                    Some(0) => y,
                    Some(_) => x,
                    None => Expr {
                        ty: x.ty.clone(),
                        kind: Kind::Cond(Box::new(condition), Box::new(x), Box::new(y)),
                        loc,
                    },
                }
            }
            ExprKind::Call(name, args) => self.call(name, args, loc),
            ExprKind::Cast(ty, operand) => {
                let to = self.ty(*ty, loc);
                if to == Ty::Void {
                    return self.wrong(loc, "nothing can be made `void`");
                }
                let value = self.rvalue(operand, Some(&to));
                self.convert(value, &to, true, loc)
            }
            ExprKind::SizeofType(ty) => {
                let ty = self.ty(*ty, loc);
                Expr {
                    kind: Kind::Const(ty.size() as i64),
                    ty: Ty::Lit,
                    loc,
                }
            }
            ExprKind::SizeofExpr(operand) => {
                // The size of an array is the size of all of it.
                let whole = match &operand.kind {
                    ExprKind::Ident(name) => match (self.lookup(name), self.globals.get(name)) {
                        (Some(Local::Var(_, ty)), _) => Some(ty.size()),
                        (None, Some(Global::Var(ty))) => Some(ty.size()),
                        _ => None,
                    },
                    _ => None,
                };
                let size = match whole {
                    Some(size) => size,
                    None => {
                        let value = self.rvalue(operand, None);
                        self.settle(value, None).ty.size()
                    }
                };
                Expr {
                    kind: Kind::Const(size as i64),
                    ty: Ty::Lit,
                    loc,
                }
            }
        }
    }

    /// An expression that is wrong in a way already reported.
    fn silent(&self, loc: Loc) -> Expr {
        Expr {
            kind: Kind::Const(0),
            ty: Ty::Error,
            loc,
        }
    }

    /// The value of a name, of `a[i]` or of `*p`.
    fn variable(&mut self, e: &ast::Expr) -> Expr {
        let loc = e.loc;
        if let ExprKind::Ident(name) = &e.kind {
            // Names that are values but not places.
            match (self.lookup(name), self.globals.get(name)) {
                (Some(Local::Const(value)), _) | (None, Some(Global::Const(value))) => {
                    return Expr {
                        loc,
                        ..value.clone()
                    };
                }
                (None, Some(Global::Function { .. })) => {
                    return self
                        .wrong(loc, format!("`{name}` is a function; call it: `{name}(…)`"));
                }
                _ => {}
            }
        }
        let Some((place, ty)) = self.place(e) else {
            return self.silent(loc);
        };
        match ty {
            // An array is used through the address of its first element.
            Ty::Array(elem, _) => {
                let kind = match place {
                    Place::Var(Var::Global(symbol)) => Kind::Sym(symbol, 0),
                    Place::Var(var) => Kind::Frame(var),
                    Place::Deref(pointer) => pointer.kind,
                };
                Expr {
                    kind,
                    ty: Ty::Ptr(elem),
                    loc,
                }
            }
            ty => Self::load(place, ty, loc),
        }
    }

    /// Something that can be assigned to, and its type.
    fn place(&mut self, e: &ast::Expr) -> Option<(Place, Ty)> {
        let loc = e.loc;
        match &e.kind {
            ExprKind::Ident(name) => {
                let global = match self.lookup(name) {
                    Some(Local::Var(var, ty)) => {
                        return Some((Place::Var(var.clone()), ty.clone()));
                    }
                    Some(Local::Const(value)) => Some(Global::Const(value.clone())),
                    None => self.globals.get(name).cloned(),
                };
                match global {
                    Some(Global::Var(ty)) => {
                        Some((Place::Var(Var::Global(format!("_{name}"))), ty))
                    }
                    Some(Global::Const(_)) => {
                        self.error(
                            loc,
                            format!("`{name}` is a constant; it has no place in memory"),
                        );
                        None
                    }
                    Some(Global::Function { .. }) => {
                        self.error(loc, format!("`{name}` is a function"));
                        None
                    }
                    None => {
                        self.error(loc, format!("`{name}` is not declared"));
                        None
                    }
                }
            }
            ExprKind::Unary(UnOp::Deref, pointer) => {
                let pointer = self.rvalue(pointer, None);
                match pointer.ty.clone() {
                    Ty::Ptr(to) => Some((Place::Deref(pointer), *to)),
                    Ty::Error => None,
                    other => {
                        let what = if other == Ty::Lit {
                            "a number".into()
                        } else {
                            a(&other)
                        };
                        self.error(loc, format!("`*` follows a pointer, and this is {what}"));
                        None
                    }
                }
            }
            ExprKind::Index(array, index) => {
                let array = self.rvalue(array, None);
                let index = self.rvalue(index, None);
                let Ty::Ptr(elem) = array.ty.clone() else {
                    if array.ty != Ty::Error {
                        self.error(loc, "only an array or a pointer can be indexed");
                    }
                    return None;
                };
                let pointer = self.offset(array, index, false, loc)?;
                Some((Place::Deref(pointer), *elem))
            }
            _ => {
                self.error(
                    loc,
                    "this is not a place in memory: it cannot be assigned or pointed at",
                );
                None
            }
        }
    }

    /// `pointer + index` (or `-`), in units of what the pointer points at.
    fn offset(&mut self, pointer: Expr, index: Expr, minus: bool, loc: Loc) -> Option<Expr> {
        let Ty::Ptr(elem) = pointer.ty.clone() else {
            return None;
        };
        let index = match &index.ty {
            Ty::Error => return None,
            Ty::Lit => {
                let v = index.constant().unwrap_or(0);
                if !bits(&Ty::U16).contains(&v) {
                    self.error(index.loc, format!("{v} does not fit in 16 bits"));
                    return None;
                }
                Self::retype(index, &Ty::U16)
            }
            ty if ty.is_int() && !ty.wide() => {
                let to = if ty.signed() { Ty::I16 } else { Ty::U16 };
                Self::widen(index, &to)
            }
            ty if ty.is_int() => index,
            _ => {
                self.error(index.loc, "a pointer can only be moved by a number");
                return None;
            }
        };
        let scaled = if elem.size() == 2 {
            let one = Expr {
                kind: Kind::Const(1),
                ty: Ty::U8,
                loc,
            };
            self.arithmetic(Op::Shl, index, one, loc)
        } else {
            index
        };
        let ty = pointer.ty.clone();
        let sign = if minus { -1 } else { 1 };
        // An address known now plus a distance known now is an address
        // known now.
        Some(match (&pointer.kind, scaled.constant()) {
            (Kind::Sym(symbol, at), Some(v)) => Expr {
                kind: Kind::Sym(symbol.clone(), at + sign * (v as i16 as i64)),
                ty,
                loc,
            },
            (Kind::Const(at), Some(v)) => Expr {
                kind: Kind::Const(ty.wrap(at + sign * v)),
                ty,
                loc,
            },
            _ => Expr {
                kind: Kind::Binary(
                    if minus { Op::Sub } else { Op::Add },
                    Box::new(pointer),
                    Box::new(scaled),
                ),
                ty,
                loc,
            },
        })
    }

    fn unary(&mut self, op: UnOp, operand: &ast::Expr, loc: Loc, hint: Option<&Ty>) -> Expr {
        match op {
            UnOp::Neg | UnOp::BitNot => {
                let value = self.rvalue(operand, hint);
                let v = value.constant();
                let apply = |v: i64| {
                    if op == UnOp::Neg {
                        v.wrapping_neg()
                    } else {
                        !v
                    }
                };
                let ty = value.ty.clone();
                match (&ty, v) {
                    (Ty::Error, _) => value,
                    (ty, Some(v)) if ty.is_int() || *ty == Ty::Lit => Expr {
                        kind: Kind::Const(ty.wrap(apply(v))),
                        ..value
                    },
                    (ty, None) if ty.is_int() => Expr {
                        ty: ty.clone(),
                        kind: if op == UnOp::Neg {
                            Kind::Neg(Box::new(value))
                        } else {
                            Kind::Not(Box::new(value))
                        },
                        loc,
                    },
                    (ty, _) => {
                        let sign = if op == UnOp::Neg { "-" } else { "~" };
                        let message = format!("`{sign}` needs a number, and this is {}", a(ty));
                        self.wrong(loc, message)
                    }
                }
            }
            UnOp::Not => {
                let value = self.truth(operand);
                match value.constant() {
                    _ if value.ty == Ty::Error => value,
                    Some(v) => Expr {
                        kind: Kind::Const((v == 0) as i64),
                        ty: Ty::U8,
                        loc,
                    },
                    None => Expr {
                        kind: Kind::LogNot(Box::new(value)),
                        ty: Ty::U8,
                        loc,
                    },
                }
            }
            UnOp::Addr => {
                let Some((place, ty)) = self.place(operand) else {
                    return self.silent(loc);
                };
                let to = match ty {
                    Ty::Array(elem, _) => *elem,
                    ty => ty,
                };
                let kind = match place {
                    Place::Var(Var::Global(symbol)) => Kind::Sym(symbol, 0),
                    Place::Var(var) => Kind::Frame(var),
                    Place::Deref(pointer) => pointer.kind,
                };
                Expr {
                    kind,
                    ty: Ty::Ptr(Box::new(to)),
                    loc,
                }
            }
            UnOp::PreInc | UnOp::PreDec | UnOp::PostInc | UnOp::PostDec => {
                let Some((place, ty)) = self.place(operand) else {
                    return self.silent(loc);
                };
                if !ty.scalar() {
                    return self.wrong(loc, "an array cannot be stepped; step a pointer to it");
                }
                if !place.pure() {
                    return self.wrong(
                        loc,
                        "this place is found by an expression that changes something, so it \
                         cannot be stepped in one go; work the place out first",
                    );
                }
                let unit = match &ty {
                    Ty::Ptr(to) => to.size() as i64,
                    _ => 1,
                };
                let up = matches!(op, UnOp::PreInc | UnOp::PostInc);
                let delta = if up { unit } else { -unit };
                if matches!(op, UnOp::PostInc | UnOp::PostDec) {
                    return Expr {
                        kind: Kind::PostAdd(Box::new(place), delta),
                        ty,
                        loc,
                    };
                }
                let step = Expr {
                    kind: Kind::Const(ty.wrap(delta)),
                    ty: ty.clone(),
                    loc,
                };
                let current = Self::load(place.clone(), ty.clone(), loc);
                let sum = self.arithmetic(Op::Add, current, step, loc);
                Expr {
                    kind: Kind::Store(Box::new(place), Box::new(sum)),
                    ty,
                    loc,
                }
            }
            UnOp::Deref => unreachable!("handled as a variable"),
        }
    }

    /// An operation on two operands of the same type (shifts: any count).
    fn arithmetic(&mut self, op: Op, x: Expr, y: Expr, loc: Loc) -> Expr {
        let divides = matches!(op, Op::DivU | Op::DivS | Op::ModU | Op::ModS);
        if divides && y.constant() == Some(0) {
            return self.wrong(loc, "this divides by zero");
        }
        if let (Some(vx), Some(vy)) = (x.constant(), y.constant()) {
            // A number without a type yet has no width to be shifted out
            // of: `1 << 40` is too large, not zero.
            if x.ty == Ty::Lit && op == Op::Shl && vy >= 32 && vx != 0 {
                return self.wrong(loc, "this constant is too large");
            }
            return match fold(op, vx, vy, &x.ty) {
                Some(v) if x.ty == Ty::Lit && !(-(1 << 32)..=1 << 32).contains(&v) => {
                    self.wrong(loc, "this constant is too large")
                }
                Some(v) => Expr {
                    kind: Kind::Const(v),
                    ty: x.ty,
                    loc,
                },
                None => self.wrong(loc, "this divides by zero"),
            };
        }
        Expr {
            ty: x.ty.clone(),
            kind: Kind::Binary(op, Box::new(x), Box::new(y)),
            loc,
        }
    }

    fn binary(
        &mut self,
        op: BinOp,
        x: &ast::Expr,
        y: &ast::Expr,
        loc: Loc,
        hint: Option<&Ty>,
    ) -> Expr {
        match op {
            BinOp::LogAnd | BinOp::LogOr => {
                let x = self.truth(x);
                let y = self.truth(y);
                if x.ty == Ty::Error || y.ty == Ty::Error {
                    return self.silent(loc);
                }
                let and = op == BinOp::LogAnd;
                let kind = match (x.constant(), y.constant()) {
                    (Some(a), Some(b)) if and => Kind::Const((a != 0 && b != 0) as i64),
                    (Some(a), Some(b)) => Kind::Const((a != 0 || b != 0) as i64),
                    _ if and => Kind::LogAnd(Box::new(x), Box::new(y)),
                    _ => Kind::LogOr(Box::new(x), Box::new(y)),
                };
                Expr {
                    kind,
                    ty: Ty::U8,
                    loc,
                }
            }
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let x = self.rvalue(x, None);
                let y = self.rvalue(y, None);
                // a > b is b < a; a <= b is b >= a.
                let (x, y) = if matches!(op, BinOp::Gt | BinOp::Le) {
                    (y, x)
                } else {
                    (x, y)
                };
                let Some((x, y)) = self.unify(x, y, loc, Mode::Compare) else {
                    return self.silent(loc);
                };
                let signed = x.ty.signed() || x.ty == Ty::Lit;
                let rel = match op {
                    BinOp::Eq => Rel::Eq,
                    BinOp::Ne => Rel::Ne,
                    BinOp::Lt | BinOp::Gt if signed => Rel::LtS,
                    BinOp::Lt | BinOp::Gt => Rel::LtU,
                    _ if signed => Rel::GeS,
                    _ => Rel::GeU,
                };
                let kind = match (x.constant(), y.constant()) {
                    // Constants are kept as the numbers they stand for, so
                    // comparing them is comparing numbers.
                    (Some(a), Some(b)) => Kind::Const(match rel {
                        Rel::Eq => a == b,
                        Rel::Ne => a != b,
                        Rel::LtU | Rel::LtS => a < b,
                        Rel::GeU | Rel::GeS => a >= b,
                    } as i64),
                    _ => Kind::Cmp(rel, Box::new(x), Box::new(y)),
                };
                Expr {
                    kind,
                    ty: Ty::U8,
                    loc,
                }
            }
            BinOp::Shl | BinOp::Shr => {
                let x = self.rvalue(x, hint);
                let y = self.rvalue(y, None);
                self.shift(op, x, y, loc, hint)
            }
            _ => {
                let x = self.rvalue(x, hint);
                let y = self.rvalue(y, hint);
                self.combine(op, x, y, loc, hint)
            }
        }
    }

    fn shift(&mut self, op: BinOp, x: Expr, y: Expr, loc: Loc, hint: Option<&Ty>) -> Expr {
        if x.ty == Ty::Error || y.ty == Ty::Error {
            return self.silent(loc);
        }
        // The count is a byte, whatever it was.
        let count = match (&y.ty, y.constant()) {
            (Ty::Lit, Some(n)) if !(0..=255).contains(&n) => {
                return self.wrong(y.loc, format!("nothing can be shifted by {n}"));
            }
            (Ty::Lit, _) if x.ty == Ty::Lit => y,
            (Ty::Lit, _) => Self::retype(y, &Ty::U8),
            (ty, _) if ty.is_int() && ty.wide() => Self::narrow(y, &Ty::U8),
            (ty, _) if ty.is_int() => Self::retype(y, &Ty::U8),
            (ty, _) => {
                return self.wrong(
                    y.loc,
                    format!("a shift count is a number, and this is {}", a(ty)),
                )
            }
        };
        let x = if count.ty == Ty::Lit {
            x
        } else {
            self.settle(x, hint)
        };
        if !x.ty.is_int() && x.ty != Ty::Lit {
            if x.ty == Ty::Error {
                return x;
            }
            return self.wrong(
                loc,
                format!("only numbers can be shifted, and this is {}", a(&x.ty)),
            );
        }
        let op = match op {
            BinOp::Shl => Op::Shl,
            _ if x.ty.signed() || x.ty == Ty::Lit => Op::ShrS,
            _ => Op::ShrU,
        };
        self.arithmetic(op, x, count, loc)
    }

    /// `+ - * / % & | ^` on two values.
    fn combine(&mut self, op: BinOp, x: Expr, y: Expr, loc: Loc, hint: Option<&Ty>) -> Expr {
        if matches!(op, BinOp::Shl | BinOp::Shr) {
            return self.shift(op, x, y, loc, hint);
        }
        let pointers = (matches!(x.ty, Ty::Ptr(_)), matches!(y.ty, Ty::Ptr(_)));
        if pointers != (false, false) {
            let moved = match (op, pointers) {
                (BinOp::Add, (true, false)) => self.offset(x, y, false, loc),
                (BinOp::Add, (false, true)) => self.offset(y, x, false, loc),
                (BinOp::Sub, (true, false)) => self.offset(x, y, true, loc),
                _ => {
                    return self.wrong(
                        loc,
                        "a pointer can be moved by a number (`p + n`, `p - n`) and nothing else",
                    );
                }
            };
            return moved.unwrap_or_else(|| self.silent(loc));
        }
        let mode = match op {
            BinOp::And | BinOp::Or | BinOp::Xor => Mode::Bits,
            _ => Mode::Value,
        };
        let Some((x, y)) = self.unify(x, y, loc, mode) else {
            return self.silent(loc);
        };
        let signed = x.ty.signed() || x.ty == Ty::Lit;
        let op = match op {
            BinOp::Add => Op::Add,
            BinOp::Sub => Op::Sub,
            BinOp::Mul => Op::Mul,
            BinOp::Div if signed => Op::DivS,
            BinOp::Div => Op::DivU,
            BinOp::Mod if signed => Op::ModS,
            BinOp::Mod => Op::ModU,
            BinOp::And => Op::And,
            BinOp::Or => Op::Or,
            _ => Op::Xor,
        };
        self.arithmetic(op, x, y, loc)
    }

    fn assign(
        &mut self,
        op: Option<BinOp>,
        target: &ast::Expr,
        value: &ast::Expr,
        loc: Loc,
    ) -> Expr {
        let Some((place, ty)) = self.place(target) else {
            // Still read the other side: it may have errors of its own.
            self.expr(value, None);
            return self.silent(loc);
        };
        if !ty.scalar() {
            self.expr(value, None);
            return self.wrong(
                loc,
                "an array cannot be assigned as a whole; assign its elements",
            );
        }
        let value = self.rvalue(value, Some(&ty));
        let value = match op {
            None => value,
            Some(op) => {
                if !place.pure() {
                    return self.wrong(
                        loc,
                        "this place is found by an expression that changes something, so it \
                         cannot be read and written in one go; work the place out first",
                    );
                }
                let current = Self::load(place.clone(), ty.clone(), loc);
                self.combine(op, current, value, loc, Some(&ty))
            }
        };
        let value = self.convert(value, &ty, false, loc);
        if value.ty == Ty::Error {
            return value;
        }
        Expr {
            kind: Kind::Store(Box::new(place), Box::new(value)),
            ty,
            loc,
        }
    }

    fn call(&mut self, name: &str, args: &[ast::Expr], loc: Loc) -> Expr {
        let signature = match (self.lookup(name).is_some(), self.globals.get(name).cloned()) {
            (false, Some(Global::Function { ret, params, .. })) => Some((ret, params)),
            (false, None) => {
                self.error(loc, format!("`{name}` is not declared"));
                None
            }
            _ => {
                self.error(loc, format!("`{name}` is not a function"));
                None
            }
        };
        let Some((ret, params)) = signature else {
            for arg in args {
                self.expr(arg, None);
            }
            return self.silent(loc);
        };
        if args.len() != params.len() {
            let takes = match params.len() {
                0 => "no arguments".to_string(),
                1 => "1 argument".to_string(),
                n => format!("{n} arguments"),
            };
            let given = if args.len() == 1 {
                "1 is given".to_string()
            } else {
                format!("{} are given", args.len())
            };
            return self.wrong(loc, format!("`{name}` takes {takes} and {given}"));
        }
        let mut values = Vec::new();
        for (arg, param) in args.iter().zip(&params) {
            let value = self.rvalue(arg, Some(param));
            values.push(self.convert(value, param, false, arg.loc));
        }
        if values.iter().any(|v| v.ty == Ty::Error) {
            return self.silent(loc);
        }
        Expr {
            kind: Kind::Call(format!("_{name}"), values),
            ty: ret,
            loc,
        }
    }
}
