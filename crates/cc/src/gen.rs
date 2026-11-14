//! The typed tree → assembly text.
//!
//! The generated code is simple on purpose. A value being computed is
//! always in `a` (8 bits) or in `ba` (16 bits); the second operand of an
//! operation is in `xl` or `x`; a value that has to wait is pushed on the
//! stack. `y` and `z` are scratch. Variables of a function are reached
//! from `sp` (ADR 0011).

use crate::ir::{Data, Datum, Expr, Function, Item, Kind, Op, Place, Program, Rel, Stmt, Var};
use crate::lex::Loc;
use crate::runtime::ROUTINES;
use silicon_asm::{Diagnostic, Pos};
use std::collections::BTreeSet;
use std::fmt::Write;

/// The farthest from `sp` an instruction can reach.
const REACH: u32 = 255;

pub(crate) struct Gen<'a> {
    files: &'a [String],
    out: String,
    errors: Vec<Diagnostic>,
    /// Runtime routines the program uses.
    needed: BTreeSet<&'static str>,
    labels: u32,
    /// The function being generated: its symbol, the bytes of its
    /// variables, and the label of the code that returns.
    name: String,
    frame: u32,
    exit: String,
    /// Bytes pushed since the function reserved its variables: everything
    /// reached from `sp` is this much farther away.
    depth: u32,
    /// For each loop around this point: where `continue` and `break` go.
    loops: Vec<(String, String)>,
    /// A function is told only once that its variables are out of reach.
    out_of_reach: bool,
}

/// `name`, `name+3`, `name-1`.
fn symbol(name: &str, offset: i64) -> String {
    match offset {
        0 => name.to_string(),
        1.. => format!("{name}+{offset}"),
        _ => format!("{name}{offset}"),
    }
}

/// A place with an address that is known when assembling.
fn fixed(place: &Place) -> Option<String> {
    match place {
        Place::Var(Var::Global(name)) => Some(name.clone()),
        Place::Var(_) => None,
        Place::Deref(pointer) => match &pointer.kind {
            Kind::Sym(name, offset) => Some(symbol(name, *offset)),
            Kind::Const(v) => Some(format!("{:#06x}", *v as u16)),
            _ => None,
        },
    }
}

fn in_frame(place: &Place) -> bool {
    matches!(place, Place::Var(Var::Local(_) | Var::Param(_)))
}

/// Can this be put in `x` without disturbing `a` and `b`?
fn x_loadable(e: &Expr) -> bool {
    match &e.kind {
        Kind::Const(_) | Kind::Sym(..) | Kind::Frame(_) => true,
        Kind::Load(place) => in_frame(place),
        Kind::Widen(false, inner) => matches!(&inner.kind, Kind::Load(place) if in_frame(place)),
        _ => false,
    }
}

/// Does computing this use nothing but `a` and `b`?
fn simple(e: &Expr) -> bool {
    match &e.kind {
        Kind::Const(_) | Kind::Sym(..) | Kind::Frame(_) => true,
        Kind::Load(place) => in_frame(place) || fixed(place).is_some(),
        Kind::Widen(_, inner) | Kind::Narrow(inner) => simple(inner),
        _ => false,
    }
}

/// `table[i]` with a byte for an index: the base and the index.
fn indexed(pointer: &Expr) -> Option<(&Expr, &Expr)> {
    match &pointer.kind {
        Kind::Binary(Op::Add, base, offset) if x_loadable(base) => match &offset.kind {
            Kind::Widen(false, index) => Some((base, index)),
            _ => None,
        },
        _ => None,
    }
}

/// 2ⁿ → n.
fn power_of_two(v: i64) -> Option<u32> {
    (v > 0 && v & (v - 1) == 0).then(|| v.trailing_zeros())
}

impl<'a> Gen<'a> {
    pub fn run(program: &Program, files: &'a [String]) -> Result<String, Vec<Diagnostic>> {
        let mut gen = Gen {
            files,
            out: String::new(),
            errors: Vec::new(),
            needed: BTreeSet::new(),
            labels: 0,
            name: String::new(),
            frame: 0,
            exit: String::new(),
            depth: 0,
            loops: Vec::new(),
            out_of_reach: false,
        };
        gen.program(program);
        if gen.errors.is_empty() {
            Ok(gen.out)
        } else {
            Err(gen.errors)
        }
    }

    // ───────────────────────────── text ─────────────────────────────

    /// One instruction, written `"mnemonic operands"`.
    fn ins(&mut self, text: impl AsRef<str>) {
        let text = text.as_ref();
        let (mnemonic, operands) = text.split_once(' ').unwrap_or((text, ""));
        let _ = writeln!(self.out, "        {mnemonic:<4} {operands}");
        // No trailing blanks after an instruction without operands.
        let kept = self.out.trim_end_matches([' ', '\n']).len();
        self.out.truncate(kept);
        self.out.push('\n');
    }

    fn label(&mut self, name: &str) {
        let _ = writeln!(self.out, "{name}:");
    }

    /// A label of the function, written with its full name so that it
    /// neither depends on nor disturbs the labels of any assembly the
    /// programmer wrote inside the function.
    fn new_label(&mut self) -> String {
        self.labels += 1;
        format!("{}.L{}", self.name, self.labels)
    }

    fn file_name(&self, file: usize) -> String {
        self.files[file].replace('\\', "\\\\").replace('"', "\\\"")
    }

    /// Errors in what follows belong to this place in the source.
    fn at(&mut self, loc: Loc) {
        let _ = writeln!(
            self.out,
            "        .loc \"{}\", {}, {}",
            self.file_name(loc.file),
            loc.line,
            loc.col
        );
    }

    fn need(&mut self, routine: &'static str) {
        if self.needed.insert(routine) {
            let needs = ROUTINES.iter().find(|r| r.name == routine).map(|r| r.needs);
            for other in needs.unwrap_or(&[]) {
                self.need(other);
            }
        }
    }

    fn call_runtime(&mut self, routine: &'static str) {
        self.need(routine);
        self.ins(format!("call {routine}"));
    }

    fn adjust_sp(&mut self, mut bytes: i64) {
        while bytes != 0 {
            let step = bytes.clamp(-128, 127);
            self.ins(format!("lea sp, [sp{step:+}]"));
            bytes -= step;
        }
    }

    fn push(&mut self, pair: &str) {
        self.ins(format!("push {pair}"));
        self.depth += 2;
    }

    fn pop(&mut self, pair: &str) {
        self.ins(format!("pop {pair}"));
        self.depth -= 2;
    }

    /// How far above `sp` a variable of the function is right now.
    fn slot(&mut self, var: &Var, loc: Loc) -> u32 {
        let offset = match var {
            Var::Local(offset) => offset + self.depth,
            // Above the variables and the return address.
            Var::Param(offset) => self.frame + 2 + offset + self.depth,
            Var::Global(_) => unreachable!("globals have fixed addresses"),
        };
        // The second byte of a 16-bit variable must be in reach too.
        if offset + 1 > REACH {
            if !self.out_of_reach {
                self.out_of_reach = true;
                self.errors.push(Diagnostic {
                    file: self.files[loc.file].clone(),
                    pos: Pos {
                        line: loc.line,
                        col: loc.col,
                    },
                    message: format!(
                        "the variables of this function take more room than can be reached \
                         from the stack pointer ({REACH} bytes); make the large ones global"
                    ),
                });
            }
            return 0;
        }
        offset
    }

    // ─────────────────────────── top level ───────────────────────────

    fn program(&mut self, program: &Program) {
        let start = Loc {
            file: 0,
            line: 1,
            col: 1,
        };
        let starts_with_values = program.data.iter().any(|d| !d.rom && d.init.is_some());
        let _ = writeln!(self.out, "; Compiled from {}.\n", self.files[0]);
        self.at(start);
        if starts_with_values {
            self.call_runtime("cc_init_data");
        }
        self.ins("call _main");
        self.ins("hlt");
        for item in &program.items {
            match item {
                Item::Function(f) => self.function(f),
                Item::Asm(text, loc) => self.asm(text, *loc),
            }
        }
        self.at(start);
        for routine in ROUTINES {
            if self.needed.contains(routine.name) {
                self.out.push_str(routine.text);
            }
        }
        self.data(program);
    }

    /// Assembly the programmer wrote: copied as it is, with its lines
    /// reported as the lines of the source they came from.
    fn asm(&mut self, text: &str, loc: Loc) {
        let _ = writeln!(
            self.out,
            "        .loc \"{}\", {}",
            self.file_name(loc.file),
            loc.line
        );
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn function(&mut self, f: &Function) {
        self.name.clone_from(&f.name);
        self.frame = f.frame;
        self.depth = 0;
        self.out_of_reach = false;
        self.exit = self.new_label();
        self.out.push('\n');
        self.at(f.loc);
        self.label(&f.name);
        self.adjust_sp(-(f.frame as i64));
        self.stmts(&f.body);
        // A `return` at the very end need not jump to the next line.
        let jump = format!("        jmp  {}\n", self.exit);
        if self.out.ends_with(&jump) {
            self.out.truncate(self.out.len() - jump.len());
        }
        self.at(f.end);
        let exit = self.exit.clone();
        self.label(&exit);
        self.adjust_sp(f.frame as i64);
        self.ins("ret");
    }

    fn data(&mut self, program: &Program) {
        let values = |data: &Data| data.init.clone().unwrap_or_default();
        self.out.push('\n');
        for data in program.data.iter().filter(|d| d.rom) {
            self.label(&data.symbol);
            self.datums(&values(data));
        }
        for (n, text) in program.strings.iter().enumerate() {
            self.label(&format!("cc_str{n}"));
            let bytes: Vec<Datum> = text
                .iter()
                .chain(&[0])
                .map(|&b| Datum::Byte(b as i64))
                .collect();
            self.datums(&bytes);
        }
        let ram = |with_values: bool| {
            program
                .data
                .iter()
                .filter(move |d| !d.rom && d.init.is_some() == with_values)
        };
        if ram(true).next().is_some() {
            self.label("cc_data_init");
            for data in ram(true) {
                self.datums(&values(data));
            }
        }
        self.ins(".ram");
        if ram(true).next().is_some() {
            self.label("cc_data");
            for data in ram(true) {
                self.label(&data.symbol);
                self.ins(format!(".res {}", data.size));
            }
            self.label("cc_data_end");
        }
        for data in ram(false) {
            self.label(&data.symbol);
            self.ins(format!(".res {}", data.size));
        }
        self.ins(".rom");
    }

    fn datums(&mut self, values: &[Datum]) {
        let mut bytes: Vec<String> = Vec::new();
        let flush = |gen: &mut Self, bytes: &mut Vec<String>| {
            for row in bytes.chunks(16) {
                gen.ins(format!(".byte {}", row.join(", ")));
            }
            bytes.clear();
        };
        for value in values {
            match value {
                Datum::Byte(v) => bytes.push((*v as u8).to_string()),
                Datum::Word(v) => {
                    flush(self, &mut bytes);
                    self.ins(format!(".word {}", *v as u16));
                }
                Datum::Addr(name, offset) => {
                    flush(self, &mut bytes);
                    self.ins(format!(".word {}", symbol(name, *offset)));
                }
            }
        }
        flush(self, &mut bytes);
    }

    // ─────────────────────────── statements ───────────────────────────

    fn stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Block(stmts) => self.stmts(stmts),
            Stmt::Asm(text, loc) => self.asm(text, *loc),
            Stmt::Expr(e) => {
                self.at(e.loc);
                self.expr(e, false);
            }
            Stmt::If(condition, then, otherwise) => {
                let (no, end) = (self.new_label(), self.new_label());
                self.at(condition.loc);
                self.branch(condition, &no, false);
                self.stmts(then);
                if otherwise.is_empty() {
                    self.label(&no);
                } else {
                    self.ins(format!("jmp {end}"));
                    self.label(&no);
                    self.stmts(otherwise);
                    self.label(&end);
                }
            }
            Stmt::Loop {
                before,
                body,
                step,
                after,
            } => {
                let (top, next, end) = (self.new_label(), self.new_label(), self.new_label());
                self.label(&top);
                if let Some(condition) = before {
                    self.at(condition.loc);
                    self.branch(condition, &end, false);
                }
                self.loops.push((next.clone(), end.clone()));
                self.stmts(body);
                self.loops.pop();
                self.label(&next);
                if let Some(step) = step {
                    self.at(step.loc);
                    self.expr(step, false);
                }
                match after {
                    Some(condition) => {
                        self.at(condition.loc);
                        self.branch(condition, &top, true);
                    }
                    None => self.ins(format!("jmp {top}")),
                }
                self.label(&end);
            }
            Stmt::Return(value, loc) => {
                self.at(*loc);
                if let Some(value) = value {
                    self.expr(value, true);
                }
                self.ins(format!("jmp {}", self.exit));
            }
            Stmt::Break(loc) | Stmt::Continue(loc) => {
                self.at(*loc);
                let (next, end) = self.loops.last().cloned().expect("checked: inside a loop");
                let target = if matches!(s, Stmt::Break(_)) {
                    end
                } else {
                    next
                };
                self.ins(format!("jmp {target}"));
            }
        }
    }

    // ────────────────────────── conditions ──────────────────────────

    /// Jump to `target` if the condition is true (`when`) or if it is
    /// false (`!when`); go on to the next instruction otherwise.
    fn branch(&mut self, e: &Expr, target: &str, when: bool) {
        match &e.kind {
            Kind::Const(v) => {
                if (*v != 0) == when {
                    self.ins(format!("jmp {target}"));
                }
            }
            Kind::LogNot(inner) => self.branch(inner, target, !when),
            // a && b is true if both are; false as soon as one is not.
            Kind::LogAnd(x, y) if when => {
                let skip = self.new_label();
                self.branch(x, &skip, false);
                self.branch(y, target, true);
                self.label(&skip);
            }
            Kind::LogAnd(x, y) => {
                self.branch(x, target, false);
                self.branch(y, target, false);
            }
            Kind::LogOr(x, y) if when => {
                self.branch(x, target, true);
                self.branch(y, target, true);
            }
            Kind::LogOr(x, y) => {
                let skip = self.new_label();
                self.branch(x, &skip, true);
                self.branch(y, target, false);
                self.label(&skip);
            }
            Kind::Cmp(rel, x, y) => {
                let holds = self.compare(*rel, x, y);
                let condition = if when { holds } else { opposite(holds) };
                self.ins(format!("j{condition} {target}"));
            }
            _ => {
                self.expr(e, true);
                self.ins(if e.ty.wide() { "or a, b" } else { "or a, a" });
                self.ins(format!("{} {target}", if when { "jnz" } else { "jz" }));
            }
        }
    }

    /// Compare two values, leaving the answer in the flags. Returns the
    /// condition (as in `bz`, `bnc`, …) that is true when the relation
    /// holds.
    fn compare(&mut self, rel: Rel, x: &Expr, y: &Expr) -> &'static str {
        let wide = x.ty.wide();
        match y.constant() {
            Some(0) if matches!(rel, Rel::Eq | Rel::Ne) => {
                self.expr(x, true);
                self.ins(if wide { "or a, b" } else { "or a, a" });
            }
            Some(v) => {
                self.expr(x, true);
                self.ins(format!("cmp a, {}", v as u8));
                if wide {
                    // The borrow carries into the high bytes, and `sbc`
                    // only clears Z: the flags are those of all 16 bits.
                    self.ins("mov a, b");
                    self.ins(format!("sbc a, {}", (v >> 8) as u8));
                }
            }
            None => {
                self.operands(x, y);
                self.ins("cmp a, xl");
                if wide {
                    self.ins("mov a, b");
                    self.ins("sbc a, xh");
                }
            }
        }
        match rel {
            Rel::Eq => "z",
            Rel::Ne => "nz",
            Rel::LtU => "nc",
            Rel::GeU => "c",
            Rel::LtS => "lt",
            Rel::GeS => "ge",
        }
    }

    // ────────────────────────── expressions ──────────────────────────

    /// Compute `e`. If `want`, its value is left in `a` or `ba`; if not,
    /// only what it changes matters.
    fn expr(&mut self, e: &Expr, want: bool) {
        if !want && e.pure() {
            return;
        }
        let wide = e.ty.wide();
        match &e.kind {
            Kind::Const(v) if wide => self.ins(format!("ld ba, {}", *v as u16)),
            Kind::Const(0) => self.ins("xor a, a"),
            Kind::Const(v) => self.ins(format!("ld a, {}", *v as u8)),
            Kind::Sym(name, offset) => self.ins(format!("ld ba, {}", symbol(name, *offset))),
            Kind::Frame(var) => {
                let n = self.slot(var, e.loc);
                self.ins(format!("lea ba, [sp+{n}]"));
            }
            Kind::Load(place) => self.load(place, wide, e.loc),
            Kind::Store(place, value) => self.store(place, value, want, e.loc),
            Kind::Neg(inner) => {
                self.expr(inner, true);
                if wide {
                    for line in ["xor a, 0xff", "add a, 1", "mov xl, a", "mov a, b"] {
                        self.ins(line);
                    }
                    // (xor leaves the carry alone)
                    for line in ["xor a, 0xff", "adc a, 0", "mov b, a", "mov a, xl"] {
                        self.ins(line);
                    }
                } else {
                    self.ins("xor a, 0xff");
                    self.ins("inc a");
                }
            }
            Kind::Not(inner) => {
                self.expr(inner, true);
                self.ins("xor a, 0xff");
                if wide {
                    for line in [
                        "mov xl, a",
                        "mov a, b",
                        "xor a, 0xff",
                        "mov b, a",
                        "mov a, xl",
                    ] {
                        self.ins(line);
                    }
                }
            }
            Kind::Binary(op, x, y) => self.binary(*op, x, y),
            Kind::Cmp(rel, x, y) => {
                // Loading does not disturb the flags.
                let holds = self.compare(*rel, x, y);
                let done = self.new_label();
                self.ins("ld a, 1");
                self.ins(format!("b{holds} {done}"));
                self.ins("dec a");
                self.label(&done);
            }
            Kind::LogNot(_) | Kind::LogAnd(..) | Kind::LogOr(..) => {
                let (yes, done) = (self.new_label(), self.new_label());
                self.branch(e, &yes, true);
                self.ins("xor a, a");
                self.ins(format!("jmp {done}"));
                self.label(&yes);
                self.ins("ld a, 1");
                self.label(&done);
            }
            Kind::Cond(condition, x, y) => {
                let (other, done) = (self.new_label(), self.new_label());
                self.branch(condition, &other, false);
                self.expr(x, true);
                self.ins(format!("jmp {done}"));
                self.label(&other);
                self.expr(y, true);
                self.label(&done);
            }
            Kind::Call(name, args) => {
                // Arguments go on the stack in the order written, two
                // bytes each; the caller takes them off again.
                for arg in args {
                    self.expr(arg, true);
                    self.push("ba");
                }
                self.ins(format!("call {name}"));
                let bytes = 2 * args.len() as u32;
                self.adjust_sp(bytes as i64);
                self.depth -= bytes;
            }
            Kind::Widen(signed, inner) => {
                self.expr(inner, true);
                self.ins("ld b, 0");
                if *signed {
                    self.sign_into_b();
                }
            }
            Kind::Narrow(inner) => self.expr(inner, true),
            Kind::PostAdd(place, delta) => {
                let ty = e.ty.clone();
                let part = |kind| Expr {
                    kind,
                    ty: ty.clone(),
                    loc: e.loc,
                };
                let current = part(Kind::Load(place.clone()));
                let step = part(Kind::Const(ty.wrap(*delta)));
                let sum = part(Kind::Binary(
                    Op::Add,
                    Box::new(current.clone()),
                    Box::new(step),
                ));
                let update = part(Kind::Store(place.clone(), Box::new(sum)));
                if want {
                    // The old value waits on the stack while the variable
                    // is changed.
                    self.expr(&current, true);
                    self.push("ba");
                    self.expr(&update, false);
                    self.pop("ba");
                } else {
                    self.expr(&update, false);
                }
            }
        }
    }

    /// `b` is 0; make it 0xff if `a` is negative.
    fn sign_into_b(&mut self) {
        let positive = self.new_label();
        self.ins("or a, a");
        self.ins(format!("bp {positive}"));
        self.ins("dec b");
        self.label(&positive);
    }

    /// Put a value in `x` (or `xl`) without touching `a` or `b`.
    fn load_x(&mut self, e: &Expr) {
        let wide = e.ty.wide();
        match &e.kind {
            Kind::Const(v) if wide => self.ins(format!("ld x, {}", *v as u16)),
            Kind::Const(v) => self.ins(format!("ld xl, {}", *v as u8)),
            Kind::Sym(name, offset) => self.ins(format!("ld x, {}", symbol(name, *offset))),
            Kind::Frame(var) => {
                let n = self.slot(var, e.loc);
                self.ins(format!("lea x, [sp+{n}]"));
            }
            Kind::Load(place) => {
                let Place::Var(var) = &**place else {
                    unreachable!("checked by x_loadable");
                };
                let n = self.slot(var, e.loc);
                self.ins(format!("ld xl, [sp+{n}]"));
                if wide {
                    self.ins(format!("ld xh, [sp+{}]", n + 1));
                }
            }
            Kind::Widen(false, inner) => {
                self.load_x(inner);
                self.ins("ld xh, 0");
            }
            _ => unreachable!("checked by x_loadable"),
        }
    }

    /// Put the address a pointer holds in `x`.
    fn address_in_x(&mut self, pointer: &Expr) {
        if let Some((base, index)) = indexed(pointer) {
            self.expr(index, true);
            self.load_x(base);
            self.ins("lea x, [x+a]");
        } else if x_loadable(pointer) {
            self.load_x(pointer);
        } else {
            self.expr(pointer, true);
            self.ins("mov xl, a");
            self.ins("mov xh, b");
        }
    }

    fn load(&mut self, place: &Place, wide: bool, loc: Loc) {
        if let Some(address) = fixed(place) {
            if wide {
                self.ins(format!("ld a, [{address}+1]"));
                self.ins("mov b, a");
            }
            self.ins(format!("ld a, [{address}]"));
            return;
        }
        match place {
            Place::Var(var) => {
                let n = self.slot(var, loc);
                self.ins(format!("ld a, [sp+{n}]"));
                if wide {
                    self.ins(format!("ld b, [sp+{}]", n + 1));
                }
            }
            Place::Deref(pointer) if !wide => {
                if let Some((base, index)) = indexed(pointer) {
                    // table[i]: one instruction once the two are in place.
                    self.expr(index, true);
                    self.load_x(base);
                    self.ins("ld a, [x+a]");
                } else if x_loadable(pointer) {
                    self.load_x(pointer);
                    self.ins("ld a, [x]");
                } else {
                    self.expr(pointer, true);
                    self.ins("ld a, [ba]");
                }
            }
            Place::Deref(pointer) => {
                self.address_in_x(pointer);
                for line in [
                    "ld a, [x+]",
                    "mov yl, a",
                    "ld a, [x]",
                    "mov b, a",
                    "mov a, yl",
                ] {
                    self.ins(line);
                }
            }
        }
    }

    fn store(&mut self, place: &Place, value: &Expr, want: bool, loc: Loc) {
        let wide = value.ty.wide();
        if let Some(address) = fixed(place) {
            self.expr(value, true);
            self.ins(format!("st [{address}], a"));
            if wide {
                if want {
                    self.ins("mov xl, a");
                }
                self.ins("mov a, b");
                self.ins(format!("st [{address}+1], a"));
                if want {
                    self.ins("mov a, xl");
                }
            }
            return;
        }
        match place {
            Place::Var(var) => {
                self.expr(value, true);
                let n = self.slot(var, loc);
                self.ins(format!("st [sp+{n}], a"));
                if wide {
                    self.ins(format!("st [sp+{}], b", n + 1));
                }
            }
            Place::Deref(pointer) => {
                // The address goes in x and the value in a or ba, in
                // whichever order lets neither disturb the other.
                if simple(value) {
                    self.address_in_x(pointer);
                    self.expr(value, true);
                } else if x_loadable(pointer) {
                    self.expr(value, true);
                    self.load_x(pointer);
                } else {
                    self.address_in_x(pointer);
                    self.push("x");
                    self.expr(value, true);
                    self.pop("x");
                }
                if wide {
                    self.ins("st [x+], a");
                    if want {
                        self.ins("mov yl, a");
                    }
                    self.ins("mov a, b");
                    self.ins("st [x], a");
                    if want {
                        self.ins("mov a, yl");
                    }
                } else {
                    self.ins("st [x], a");
                }
            }
        }
    }

    /// Left operand in `a`/`ba`, right operand in `xl`/`x`.
    fn operands(&mut self, x: &Expr, y: &Expr) {
        let to_x = |gen: &mut Self| {
            gen.ins("mov xl, a");
            if y.ty.wide() {
                gen.ins("mov xh, b");
            }
        };
        if x_loadable(y) {
            self.expr(x, true);
            self.load_x(y);
        } else if simple(x) {
            self.expr(y, true);
            to_x(self);
            self.expr(x, true);
        } else {
            self.expr(x, true);
            self.push("ba");
            self.expr(y, true);
            to_x(self);
            self.pop("ba");
        }
    }

    fn binary(&mut self, op: Op, x: &Expr, y: &Expr) {
        let wide = x.ty.wide();
        // Whatever is easiest to fetch goes on the right, where it can be
        // fetched last.
        let easier = x.constant().is_some() || (x_loadable(x) && !x_loadable(y));
        let (x, y) = if op.commutes() && easier && y.constant().is_none() {
            (y, x)
        } else {
            (x, y)
        };
        match y.constant() {
            Some(v) => {
                self.expr(x, true);
                let done = if wide {
                    self.with_constant16(op, v as u16)
                } else {
                    self.with_constant8(op, v as u8)
                };
                if done {
                    return;
                }
                self.load_x(y);
            }
            None => self.operands(x, y),
        }
        if wide {
            self.apply16(op);
        } else {
            self.apply8(op);
        }
    }

    fn times(&mut self, n: u32, line: &str) {
        for _ in 0..n {
            self.ins(line);
        }
    }

    /// `a ← a op v`, if there is a way that needs no second register.
    fn with_constant8(&mut self, op: Op, v: u8) -> bool {
        let power = power_of_two(v as i64);
        match (op, power) {
            (Op::Add, _) | (Op::Sub, _) => {
                let v = if op == Op::Add { v } else { v.wrapping_neg() };
                match v {
                    0 => {}
                    1 => self.ins("inc a"),
                    255 => self.ins("dec a"),
                    _ => self.ins(format!("add a, {v}")),
                }
            }
            (Op::And, _) => self.ins(format!("and a, {v}")),
            (Op::Or, _) => self.ins(format!("or a, {v}")),
            (Op::Xor, _) => self.ins(format!("xor a, {v}")),
            (Op::Shl, _) | (Op::ShrU, _) if v >= 8 => self.ins("xor a, a"),
            (Op::Shl, _) => self.times(v as u32, "add a, a"),
            (Op::ShrU, _) => self.times(v as u32, "shr a"),
            (Op::ShrS, _) => self.times(v.min(7) as u32, "asr a"),
            (Op::Mul, Some(n)) => self.times(n, "add a, a"),
            (Op::DivU, Some(n)) => self.times(n, "shr a"),
            (Op::ModU, Some(_)) => self.ins(format!("and a, {}", v - 1)),
            _ => return false,
        }
        true
    }

    /// One step of a 16-bit shift to the right: `shift` is `shr` or `asr`.
    fn shift_right16(&mut self, shift: &str) {
        for line in [
            "mov xl, a",
            "mov a, b",
            shift,
            "mov b, a",
            "mov a, xl",
            "ror a",
        ] {
            self.ins(line);
        }
    }

    /// `op a, lo` then `op b, hi`, for the operations that work byte by
    /// byte.
    fn bytewise(&mut self, op: &str, lo: &str, hi: &str) {
        self.ins(format!("{op} a, {lo}"));
        self.ins("mov xl, a");
        self.ins("mov a, b");
        self.ins(format!("{op} a, {hi}"));
        self.ins("mov b, a");
        self.ins("mov a, xl");
    }

    /// `ba ← ba op v`, if there is a way that needs no second pair.
    fn with_constant16(&mut self, op: Op, v: u16) -> bool {
        let power = power_of_two(v as i64);
        let (lo, hi) = ((v as u8).to_string(), (v >> 8).to_string());
        match (op, power) {
            (Op::Add, _) | (Op::Sub, _) => {
                let v = if op == Op::Add { v } else { v.wrapping_neg() };
                match v as i16 {
                    0 => {}
                    d @ -128..=127 => self.ins(format!("lea ba, [ba{d:+}]")),
                    _ => {
                        self.ins(format!("ld x, {v}"));
                        self.ins("add ba, x");
                    }
                }
            }
            (Op::And, _) => self.bytewise("and", &lo, &hi),
            (Op::Or, _) => self.bytewise("or", &lo, &hi),
            (Op::Xor, _) => self.bytewise("xor", &lo, &hi),
            (Op::Shl, _) | (Op::ShrU, _) if v >= 16 => self.ins("ld ba, 0"),
            (Op::Shl, _) => self.shift_left16(v as u32),
            (Op::ShrU, _) if v >= 8 => {
                self.ins("mov a, b");
                self.ins("ld b, 0");
                self.times(v as u32 - 8, "shr a");
            }
            (Op::ShrU, _) => {
                for _ in 0..v {
                    self.shift_right16("shr a");
                }
            }
            (Op::ShrS, _) if v >= 8 => {
                self.ins("mov a, b");
                self.ins("ld b, 0");
                self.sign_into_b();
                self.times(v.min(15) as u32 - 8, "asr a");
            }
            (Op::ShrS, _) => {
                for _ in 0..v {
                    self.shift_right16("asr a");
                }
            }
            (Op::Mul, Some(n)) => self.shift_left16(n),
            (Op::DivU, Some(n)) => return self.with_constant16(Op::ShrU, n as u16),
            (Op::ModU, Some(_)) => return self.with_constant16(Op::And, v - 1),
            _ => return false,
        }
        true
    }

    fn shift_left16(&mut self, n: u32) {
        if n >= 8 {
            self.ins("mov b, a");
            self.ins("ld a, 0");
            self.times(n - 8, "add ba, ba");
        } else {
            self.times(n, "add ba, ba");
        }
    }

    /// `a ← a op xl`
    fn apply8(&mut self, op: Op) {
        match op {
            Op::Add => self.ins("add a, xl"),
            Op::Sub => self.ins("sub a, xl"),
            Op::And => self.ins("and a, xl"),
            Op::Or => self.ins("or a, xl"),
            Op::Xor => self.ins("xor a, xl"),
            Op::Mul => self.call_runtime("cc_mul8"),
            Op::DivU => self.call_runtime("cc_udivmod8"),
            Op::DivS => self.call_runtime("cc_sdivmod8"),
            Op::ModU | Op::ModS => {
                self.call_runtime(if op == Op::ModU {
                    "cc_udivmod8"
                } else {
                    "cc_sdivmod8"
                });
                self.ins("mov a, xh");
            }
            Op::Shl => self.call_runtime("cc_shl8"),
            Op::ShrU => self.call_runtime("cc_shr8"),
            Op::ShrS => self.call_runtime("cc_sar8"),
        }
    }

    /// `ba ← ba op x` (shifts: by `xl`)
    fn apply16(&mut self, op: Op) {
        match op {
            Op::Add => self.ins("add ba, x"),
            Op::Sub => {
                // Low bytes, then high bytes with the borrow.
                for line in [
                    "sub a, xl",
                    "mov xl, a",
                    "mov a, b",
                    "sbc a, xh",
                    "mov b, a",
                    "mov a, xl",
                ] {
                    self.ins(line);
                }
            }
            Op::And => self.bytewise("and", "xl", "xh"),
            Op::Or => self.bytewise("or", "xl", "xh"),
            Op::Xor => self.bytewise("xor", "xl", "xh"),
            Op::Mul => self.call_runtime("cc_mul16"),
            Op::DivU => self.call_runtime("cc_udivmod16"),
            Op::DivS => self.call_runtime("cc_sdivmod16"),
            Op::ModU | Op::ModS => {
                self.call_runtime(if op == Op::ModU {
                    "cc_udivmod16"
                } else {
                    "cc_sdivmod16"
                });
                self.ins("mov a, yl");
                self.ins("mov b, yh");
            }
            Op::Shl => self.call_runtime("cc_shl16"),
            Op::ShrU => self.call_runtime("cc_shr16"),
            Op::ShrS => self.call_runtime("cc_sar16"),
        }
    }
}

/// The condition that is true exactly when `condition` is not.
fn opposite(condition: &'static str) -> &'static str {
    match condition {
        "z" => "nz",
        "nz" => "z",
        "c" => "nc",
        "nc" => "c",
        "lt" => "ge",
        _ => "lt",
    }
}
