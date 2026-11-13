//! From source files to a ROM image.
//!
//! 1. **Load**: read the main file and everything it includes, line by line,
//!    into one flat list of statements.
//! 2. **Lay out**: walk the list giving every statement an address. The only
//!    statement whose size is not known from its text is a conditional jump
//!    (`jz` …), which is 2 bytes when its target is near and 5 when it is
//!    not. They all start short; any that cannot reach is made long and the
//!    walk is repeated. A jump never shrinks again, so this ends.
//! 3. **Emit**: evaluate every operand with the final addresses, check its
//!    range and write the bytes.

use crate::expr::Expr;
use crate::lex::{lex, LineError};
use crate::parse::{operand_of, parse_line, with_operand, Datum, Operand, Stmt, Value};
use crate::{Assembly, Diagnostic, Files, Pos, SourceLine, RAM_START, ROM_SIZE};
use silicon_isa::{Cond, Insn};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

/// Stop reporting after this many errors: one mistake can cause hundreds.
const MAX_ERRORS: usize = 50;
/// How deep `.include` may nest before it is taken for a loop.
const MAX_INCLUDE_DEPTH: usize = 16;
/// Size of a conditional jump in its two encodings.
const SHORT_JUMP: u32 = 2;
const LONG_JUMP: u32 = 5;

/// One statement with where it came from and where it ends up.
struct Unit {
    stmt: Stmt,
    /// Reported position (after `.loc`).
    file: usize,
    line: u32,
    col: u32,
    /// The position was set by a `.loc` with a column: every error in this
    /// statement is reported there, whatever its own column.
    fixed_col: bool,
    /// Index into `Assembler::lines`.
    source: usize,
    /// Address given by the last layout pass (for `.org`, the new address).
    addr: u32,
    /// The layout pass that last gave it an address.
    pass: u32,
    /// For `Stmt::Jump`: encoded as a branch around a `jmp`.
    long: bool,
}

enum Symbol {
    /// A label and the layout pass that last placed it.
    Label { addr: u32, pass: u32 },
    /// `name = expr`, by the index of its unit.
    Const { unit: usize },
}

struct Defined {
    symbol: Symbol,
    file: usize,
    line: u32,
}

/// Which symbols an expression may use.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// While laying out: only labels already placed in this pass, because
    /// the answer decides where everything after it goes.
    Layout,
    /// After layout: everything.
    Final,
}

pub(crate) struct Assembler<'a> {
    files: &'a dyn Files,
    file_names: Vec<String>,
    units: Vec<Unit>,
    lines: Vec<SourceLine>,
    symbols: HashMap<String, Defined>,
    errors: Vec<Diagnostic>,
    pass: u32,
    /// The constants whose values are being computed right now (by unit),
    /// to notice one that is defined in terms of itself.
    computing: RefCell<Vec<usize>>,
}

impl<'a> Assembler<'a> {
    pub fn new(files: &'a dyn Files) -> Self {
        Assembler {
            files,
            file_names: Vec::new(),
            units: Vec::new(),
            lines: Vec::new(),
            symbols: HashMap::new(),
            errors: Vec::new(),
            pass: 0,
            computing: RefCell::new(Vec::new()),
        }
    }

    pub fn run(mut self, main: &str) -> Result<Assembly, Vec<Diagnostic>> {
        match self.files.read(main) {
            Ok(text) => {
                let mut scope = String::new();
                self.load(main, &text, &mut scope, 0);
            }
            Err(why) => {
                let file = self.file(main);
                self.error(file, 1, 1, format!("cannot read `{main}`: {why}"));
            }
        }
        if self.errors.is_empty() {
            self.declare();
        }
        if self.errors.is_empty() {
            self.layout();
        }
        if !self.errors.is_empty() {
            return Err(self.errors);
        }
        let (rom, lengths) = self.emit();
        if !self.errors.is_empty() {
            return Err(self.errors);
        }

        // Each line starts at its first statement and holds what they emit.
        for (unit, len) in self.units.iter().zip(lengths) {
            let line = &mut self.lines[unit.source];
            if line.len == 0 && !matches!(unit.stmt, Stmt::Const(..)) {
                line.addr = Some(unit.addr.min(0xFFFF) as u16);
            }
            line.len += len;
        }
        let mut symbols = BTreeMap::new();
        for name in self.symbols.keys() {
            if let Ok(value) = self.symbol(name, Mode::Final) {
                symbols.insert(name.clone(), value);
            }
        }
        Ok(Assembly {
            rom,
            symbols,
            files: self.file_names,
            lines: self.lines,
        })
    }

    fn file(&mut self, name: &str) -> usize {
        if let Some(i) = self.file_names.iter().position(|n| n == name) {
            return i;
        }
        self.file_names.push(name.to_string());
        self.file_names.len() - 1
    }

    fn error(&mut self, file: usize, line: u32, col: u32, message: String) {
        if self.errors.len() < MAX_ERRORS {
            self.errors.push(Diagnostic {
                file: self.file_names[file].clone(),
                pos: Pos { line, col },
                message,
            });
        } else if self.errors.len() == MAX_ERRORS {
            self.errors.push(Diagnostic {
                file: self.file_names[file].clone(),
                pos: Pos { line, col },
                message: "too many errors; stopping here".into(),
            });
        }
    }

    fn error_at(&mut self, unit: usize, (col, message): LineError) {
        let u = &self.units[unit];
        let (file, line) = (u.file, u.line);
        let col = if u.fixed_col { u.col } else { col };
        self.error(file, line, col, message);
    }

    // ───────────────────────────── load ─────────────────────────────

    /// Read `text` (the contents of `name`) into units, following includes.
    fn load(&mut self, name: &str, text: &str, scope: &mut String, depth: usize) {
        let physical = self.file(name);
        // `.loc` redirects the reported position of the lines after it:
        // line for line, or all to one fixed place (when it gives a column).
        let mut reported: Option<(usize, u32, Option<u32>)> = None;
        for (index, text) in text.lines().enumerate() {
            let (file, line, fixed_col) = match &mut reported {
                Some((file, line, col)) => {
                    let here = (*file, *line, *col);
                    if col.is_none() {
                        *line += 1;
                    }
                    here
                }
                None => (physical, index as u32 + 1, None),
            };
            let source = self.lines.len();
            self.lines.push(SourceLine {
                file: physical,
                line: index as u32 + 1,
                text: text.to_string(),
                addr: None,
                len: 0,
            });
            let parsed = lex(text).and_then(|toks| parse_line(&toks, text.len(), scope));
            let stmts = match parsed {
                Ok(stmts) => stmts,
                Err((col, message)) => {
                    self.error(file, line, fixed_col.unwrap_or(col), message);
                    continue;
                }
            };
            for located in stmts {
                let col = fixed_col.unwrap_or(located.col);
                match located.stmt {
                    Stmt::Loc(to, to_line, to_col) => {
                        reported = Some((self.file(&to), to_line, to_col));
                    }
                    Stmt::Include(path) => {
                        let path = resolve(name, &path);
                        if depth >= MAX_INCLUDE_DEPTH {
                            let message = format!("`{path}` is included from inside itself");
                            self.error(file, line, col, message);
                            continue;
                        }
                        match self.files.read(&path) {
                            Ok(included) => self.load(&path, &included, scope, depth + 1),
                            Err(why) => {
                                let message = format!("cannot read `{path}`: {why}");
                                self.error(file, line, col, message);
                            }
                        }
                    }
                    stmt => self.units.push(Unit {
                        stmt,
                        file,
                        line,
                        col,
                        fixed_col: fixed_col.is_some(),
                        source,
                        addr: 0,
                        pass: 0,
                        long: false,
                    }),
                }
            }
        }
    }

    /// Enter every label and constant in the symbol table.
    fn declare(&mut self) {
        for i in 0..self.units.len() {
            let unit = &self.units[i];
            let (name, symbol) = match &unit.stmt {
                Stmt::Label(name) => (name, Symbol::Label { addr: 0, pass: 0 }),
                Stmt::Const(name, _) => (name, Symbol::Const { unit: i }),
                _ => continue,
            };
            if let Some(first) = self.symbols.get(name) {
                let message = format!(
                    "`{name}` is already defined at {}:{}",
                    self.file_names[first.file], first.line
                );
                self.error_at(i, (unit.col, message));
                continue;
            }
            let defined = Defined {
                symbol,
                file: unit.file,
                line: unit.line,
            };
            self.symbols.insert(name.clone(), defined);
        }
    }

    // ──────────────────────────── values ────────────────────────────

    fn eval(&self, expr: &Expr, here: u32, mode: Mode) -> Result<i64, LineError> {
        expr.eval(here as i64, &mut |name| self.symbol(name, mode))
    }

    fn symbol(&self, name: &str, mode: Mode) -> Result<i64, String> {
        let Some(defined) = self.symbols.get(name) else {
            return Err(format!("`{name}` is not defined"));
        };
        match defined.symbol {
            Symbol::Label { addr, pass } => {
                if mode == Mode::Layout && pass != self.pass {
                    return Err(format!(
                        "`{name}` is defined further down, so its address is not known yet here"
                    ));
                }
                Ok(addr as i64)
            }
            Symbol::Const { unit } => self.constant(unit, mode).map_err(|(_, message)| {
                // Say which constant led here, unless the message is about it
                // or about a circle of them.
                if message.starts_with(&format!("`{name}` ")) || message.ends_with(CIRCULAR) {
                    message
                } else {
                    format!("{message} (needed by `{name}`)")
                }
            }),
        }
    }

    /// The value of the constant defined by unit `i`. The column of an error
    /// is a column of the constant's own line.
    fn constant(&self, i: usize, mode: Mode) -> Result<i64, LineError> {
        let unit = &self.units[i];
        let Stmt::Const(name, expr) = &unit.stmt else {
            unreachable!("constant symbols point at constant statements");
        };
        if self.computing.borrow().contains(&i) {
            return Err((unit.col, format!("`{name}` {CIRCULAR}")));
        }
        // `$` in a constant is the address where it is written.
        if mode == Mode::Layout && unit.pass != self.pass && expr.mentions_here() {
            return Err((
                unit.col,
                format!(
                    "`{name}` uses `$` and is defined further down, so its value is not known \
                     yet here"
                ),
            ));
        }
        self.computing.borrow_mut().push(i);
        let value = self.eval(expr, unit.addr, mode);
        self.computing.borrow_mut().pop();
        value
    }

    // ──────────────────────────── layout ────────────────────────────

    fn layout(&mut self) {
        loop {
            self.pass += 1;
            // Two location counters: the one in use, and the one of the
            // other part of memory, waiting for `.rom` or `.ram`.
            let mut pc: u32 = 0;
            let mut parked: u32 = RAM_START as u32;
            let mut in_ram = false;
            for i in 0..self.units.len() {
                if let Stmt::Section(ram) = self.units[i].stmt {
                    if ram != in_ram {
                        std::mem::swap(&mut pc, &mut parked);
                        in_ram = ram;
                    }
                }
                let size = match self.size(i, pc) {
                    Ok(Next::Skip(size)) => size,
                    Ok(Next::At(addr)) => {
                        pc = addr;
                        0
                    }
                    Err(e) => {
                        self.error_at(i, e);
                        0
                    }
                };
                self.units[i].addr = pc;
                self.units[i].pass = self.pass;
                if let Stmt::Label(name) = &self.units[i].stmt {
                    let pass = self.pass;
                    if let Some(defined) = self.symbols.get_mut(name) {
                        defined.symbol = Symbol::Label { addr: pc, pass };
                    }
                }
                pc += size;
                if pc > 0x1_0000 {
                    let col = self.units[i].col;
                    self.error_at(i, (col, "this runs past the end of memory (0xffff)".into()));
                    return;
                }
            }
            if !self.errors.is_empty() {
                return;
            }

            // Lengthen the jumps that cannot reach, and go round again.
            let mut grew = false;
            for i in 0..self.units.len() {
                let unit = &self.units[i];
                let Stmt::Jump(_, target) = &unit.stmt else {
                    continue;
                };
                if unit.long {
                    continue;
                }
                // An undefined target is reported when emitting.
                if let Ok(to) = self.eval(&target.expr, unit.addr, Mode::Final) {
                    let distance = to - (unit.addr as i64 + SHORT_JUMP as i64);
                    if i8::try_from(distance).is_err() {
                        self.units[i].long = true;
                        grew = true;
                    }
                }
            }
            if !grew {
                return;
            }
        }
    }

    /// How a statement moves the location counter.
    fn size(&self, i: usize, pc: u32) -> Result<Next, LineError> {
        let unit = &self.units[i];
        let count = |v: &Value, what: &str, max: i64| -> Result<u32, LineError> {
            let n = self.eval(&v.expr, pc, Mode::Layout)?;
            if !(0..=max).contains(&n) {
                return Err((v.col, format!("{what} is {n}; it must be 0…{max}")));
            }
            Ok(n as u32)
        };
        Ok(Next::Skip(match &unit.stmt {
            Stmt::Label(_)
            | Stmt::Const(..)
            | Stmt::Include(_)
            | Stmt::Loc(..)
            | Stmt::Section(_) => 0,
            Stmt::Org(v) => return Ok(Next::At(count(v, "the address", 0xFFFF)?)),
            Stmt::Res(v) => count(v, "the size", 0x1_0000)?,
            Stmt::Fill(v, _) => count(v, "the count", 0x1_0000)?,
            Stmt::Align(v) => {
                let n = self.eval(&v.expr, pc, Mode::Layout)?;
                if !(1..=0x1_0000).contains(&n) {
                    return Err((
                        v.col,
                        format!("the alignment is {n}; it must be at least 1"),
                    ));
                }
                let n = n as u32;
                pc.next_multiple_of(n) - pc
            }
            Stmt::Byte(data) => data
                .iter()
                .map(|d| match d {
                    Datum::Value(_) => 1,
                    Datum::Text(text) => text.len() as u32,
                })
                .sum(),
            Stmt::Word(data) => 2 * data.len() as u32,
            Stmt::Insn(insn, _) => insn.len() as u32,
            Stmt::Jump(..) if unit.long => LONG_JUMP,
            Stmt::Jump(..) => SHORT_JUMP,
        }))
    }

    // ───────────────────────────── emit ─────────────────────────────

    /// Write every byte. Returns the image and how many bytes each unit
    /// wrote.
    fn emit(&mut self) -> (Vec<u8>, Vec<u16>) {
        let mut rom = vec![0u8; ROM_SIZE];
        let mut written = vec![false; ROM_SIZE];
        let mut end = 0;
        let mut lengths = Vec::with_capacity(self.units.len());
        let mut bytes = Vec::new();
        for i in 0..self.units.len() {
            bytes.clear();
            if let Err(e) = self.encode(i, &mut bytes) {
                self.error_at(i, e);
            }
            lengths.push(bytes.len() as u16);
            if bytes.is_empty() {
                continue;
            }
            let start = self.units[i].addr as usize;
            let col = self.units[i].col;
            if start + bytes.len() > ROM_SIZE {
                let message = format!(
                    "this would be stored at {:#06x}, outside the ROM (0x0000–{:#06x}); \
                     RAM cannot be given contents by a ROM image (use `.res` to name it)",
                    start.max(ROM_SIZE),
                    ROM_SIZE - 1
                );
                self.error_at(i, (col, message));
                continue;
            }
            if let Some(taken) = written[start..start + bytes.len()].iter().position(|&w| w) {
                let message = format!("this overwrites what is already at {:#06x}", start + taken);
                self.error_at(i, (col, message));
                continue;
            }
            rom[start..start + bytes.len()].copy_from_slice(&bytes);
            written[start..start + bytes.len()].fill(true);
            end = end.max(start + bytes.len());
        }
        rom.truncate(end);
        (rom, lengths)
    }

    fn encode(&self, i: usize, out: &mut Vec<u8>) -> Result<(), LineError> {
        let unit = &self.units[i];
        let here = unit.addr;
        let number = |v: &Value, what: &str, min: i64, max: i64| -> Result<i64, LineError> {
            let n = self.eval(&v.expr, here, Mode::Final)?;
            if !(min..=max).contains(&n) {
                return Err((v.col, format!("{what} is {n}; it must be {min}…{max}")));
            }
            Ok(n)
        };
        let branch = |cond: Cond, from: u32, target: &Value| -> Result<Insn, LineError> {
            let to = number(target, "the target address", 0, 0xFFFF)?;
            let distance = to - (from as i64 + 2);
            let offset = i8::try_from(distance).map_err(|_| {
                (
                    target.col,
                    format!(
                        "the target is {distance} bytes away and a branch reaches −128…+127; \
                         write `j{}` to let the assembler use a `jmp`",
                        &cond.name()[1..]
                    ),
                )
            })?;
            Ok(Insn::Br(cond, offset))
        };
        match &unit.stmt {
            // Checked here so that a constant nobody uses is still an error
            // when it cannot be computed.
            Stmt::Const(..) => {
                self.constant(i, Mode::Final)?;
            }
            Stmt::Label(_)
            | Stmt::Include(_)
            | Stmt::Loc(..)
            | Stmt::Section(_)
            | Stmt::Org(_)
            | Stmt::Res(_)
            | Stmt::Align(_) => {}
            Stmt::Fill(_, byte) => {
                let size = match self.size(i, here)? {
                    Next::Skip(size) => size,
                    Next::At(_) => 0,
                };
                let byte = match byte {
                    Some(v) => number(v, "the byte", -128, 255)?,
                    None => 0,
                };
                out.resize(size as usize, byte as u8);
            }
            Stmt::Byte(data) => {
                for d in data {
                    match d {
                        Datum::Value(v) => out.push(number(v, "the byte", -128, 255)? as u8),
                        Datum::Text(text) => out.extend_from_slice(text),
                    }
                }
            }
            Stmt::Word(data) => {
                for v in data {
                    let n = number(v, "the word", -32768, 65535)?;
                    out.extend_from_slice(&(n as u16).to_le_bytes());
                }
            }
            Stmt::Insn(insn, None) => insn.encode(out),
            Stmt::Insn(insn, Some(v)) => {
                let insn = match (operand_of(insn), *insn) {
                    (Operand::Branch, Insn::Br(cond, _)) => branch(cond, here, v)?,
                    (kind, insn) => {
                        let n = match kind {
                            Operand::Byte => number(v, "the value", -128, 255)?,
                            Operand::SpOffset => number(v, "the offset from `sp`", 0, 255)?,
                            Operand::Signed => number(v, "the offset", -128, 127)?,
                            _ => number(v, "the value", -32768, 65535)?,
                        };
                        with_operand(insn, n)
                    }
                };
                insn.encode(out);
            }
            Stmt::Jump(cond, target) if unit.long => {
                // The opposite condition skips over the `jmp`.
                let opposite = Cond::from_bits(*cond as u8 ^ 1);
                Insn::Br(opposite, 3).encode(out);
                let to = number(target, "the target address", 0, 0xFFFF)?;
                Insn::Jmp(to as u16).encode(out);
            }
            Stmt::Jump(cond, target) => branch(*cond, here, target)?.encode(out),
        }
        Ok(())
    }
}

/// What is wrong with `a = b` when `b = a`.
const CIRCULAR: &str = "is defined in terms of itself";

/// The effect of a statement on the location counter.
enum Next {
    /// Advance by this many bytes.
    Skip(u32),
    /// Continue at this address.
    At(u32),
}

/// The path of an included file: relative to the directory of the file that
/// includes it, with `.` and `..` resolved. A path that begins with `/` is
/// relative to the root of the [`Files`].
pub fn resolve(from: &str, path: &str) -> String {
    let mut parts: Vec<&str> = match path.strip_prefix('/') {
        Some(_) => Vec::new(),
        None => {
            let mut dir: Vec<&str> = from.split('/').collect();
            dir.pop();
            dir
        }
    };
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::resolve;

    #[test]
    fn include_paths_are_relative_to_the_including_file() {
        assert_eq!(resolve("main.asm", "io.inc"), "io.inc");
        assert_eq!(
            resolve("maze3d/main.asm", "tables.inc"),
            "maze3d/tables.inc"
        );
        assert_eq!(resolve("maze3d/main.asm", "../lib/io.inc"), "lib/io.inc");
        assert_eq!(resolve("a/b/c.asm", "./d.asm"), "a/b/d.asm");
        assert_eq!(resolve("a/b/c.asm", "/lib/io.inc"), "lib/io.inc");
    }
}
