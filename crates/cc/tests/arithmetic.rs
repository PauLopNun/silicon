//! The arithmetic the CPU has no instruction for (multiplying, dividing,
//! shifting by a computed count) against Rust's own: every pair of 8-bit
//! operands, and 16-bit pairs chosen to include the awkward ones.
//!
//! The operands are put in the program's variables by hand, so the compiler
//! cannot work anything out in advance: the routines of `runtime.rs` do it.

mod common;

use silicon_machine::{Bus, Machine};

const PROGRAM: &str = "
    u8 ua; u8 ub; i8 sa; i8 sb;
    u8 mul; u8 div; u8 mod; i8 smul; i8 sdiv; i8 smod;
    u8 shl; u8 shr; i8 sar;

    u16 wa; u16 wb; i16 ta; i16 tb;
    u16 wmul; u16 wdiv; u16 wmod; i16 tmul; i16 tdiv; i16 tmod;
    u16 wshl; u16 wshr; i16 tsar;

    void main() {
        mul = ua * ub; div = ua / ub; mod = ua % ub;
        smul = sa * sb; sdiv = sa / sb; smod = sa % sb;
        shl = ua << ub; shr = ua >> ub; sar = sa >> ub;

        wmul = wa * wb; wdiv = wa / wb; wmod = wa % wb;
        tmul = ta * tb; tdiv = ta / tb; tmod = ta % tb;
        wshl = wa << ub; wshr = wa >> ub; tsar = ta >> ub;
    }
";

struct Rig {
    program: silicon_asm::Assembly,
    machine: Machine,
}

impl Rig {
    fn new() -> Rig {
        let program = common::build(PROGRAM);
        let machine = Machine::new(&program.rom).expect("the program fits in ROM");
        Rig { program, machine }
    }

    fn at(&self, name: &str) -> u16 {
        self.program.symbols[&format!("_{name}")] as u16
    }

    fn set8(&mut self, name: &str, value: u8) {
        let at = self.at(name);
        self.machine.board_mut().write(at, value);
    }

    fn set16(&mut self, name: &str, value: u16) {
        let at = self.at(name);
        let [lo, hi] = value.to_le_bytes();
        self.machine.board_mut().write(at, lo);
        self.machine.board_mut().write(at + 1, hi);
    }

    fn get8(&self, name: &str) -> u8 {
        self.machine.board().peek(self.at(name))
    }

    fn get16(&self, name: &str) -> u16 {
        let at = self.at(name);
        let board = self.machine.board();
        u16::from_le_bytes([board.peek(at), board.peek(at + 1)])
    }

    /// Run `main` with these operands.
    fn run(&mut self, a8: u8, b8: u8, a16: u16, b16: u16) {
        self.machine.reset();
        self.set8("ua", a8);
        self.set8("ub", b8);
        self.set8("sa", a8);
        self.set8("sb", b8);
        self.set16("wa", a16);
        self.set16("wb", b16);
        self.set16("ta", a16);
        self.set16("tb", b16);
        self.machine.run_until_halt(100_000);
        assert!(
            self.machine.halted(),
            "{a8} {b8} {a16} {b16}: still running"
        );
    }
}

// What the language says the results are (docs/LANGUAGE.md). Dividing by
// zero is defined, like everything else in this computer: the quotient is
// all ones for unsigned numbers, -1 or 1 (against the sign of the dividend)
// for signed ones, and the remainder is the dividend.

fn udiv8(a: u8, b: u8) -> (u8, u8) {
    match b {
        0 => (255, a),
        _ => (a / b, a % b),
    }
}

fn sdiv8(a: i8, b: i8) -> (i8, i8) {
    match b {
        0 => (if a < 0 { 1 } else { -1 }, a),
        _ => (a.wrapping_div(b), a.wrapping_rem(b)),
    }
}

fn udiv16(a: u16, b: u16) -> (u16, u16) {
    match b {
        0 => (65535, a),
        _ => (a / b, a % b),
    }
}

fn sdiv16(a: i16, b: i16) -> (i16, i16) {
    match b {
        0 => (if a < 0 { 1 } else { -1 }, a),
        _ => (a.wrapping_div(b), a.wrapping_rem(b)),
    }
}

/// Shifts by a count as large as the type shift everything out.
fn shl(a: u16, n: u8, bits: u8) -> u16 {
    if n >= bits {
        0
    } else {
        a << n
    }
}

fn shr(a: u16, n: u8, bits: u8) -> u16 {
    if n >= bits {
        0
    } else {
        a >> n
    }
}

fn sar(a: i16, n: u8) -> i16 {
    a >> n.min(15)
}

fn check8(rig: &Rig, a: u8, b: u8) {
    let (sa, sb) = (a as i8, b as i8);
    let got = [
        rig.get8("mul"),
        rig.get8("div"),
        rig.get8("mod"),
        rig.get8("smul"),
        rig.get8("sdiv"),
        rig.get8("smod"),
        rig.get8("shl"),
        rig.get8("shr"),
        rig.get8("sar"),
    ];
    let (div, rem) = udiv8(a, b);
    let (sdiv, srem) = sdiv8(sa, sb);
    let want = [
        a.wrapping_mul(b),
        div,
        rem,
        sa.wrapping_mul(sb) as u8,
        sdiv as u8,
        srem as u8,
        shl(a as u16, b, 8) as u8,
        shr(a as u16, b, 8) as u8,
        sar(sa as i16, b) as u8,
    ];
    assert_eq!(
        got, want,
        "{a} and {b} ({sa} and {sb}): * / % signed * / % << >> signed >>"
    );
}

fn check16(rig: &Rig, a: u16, b: u16, count: u8) {
    let (ta, tb) = (a as i16, b as i16);
    let got = [
        rig.get16("wmul"),
        rig.get16("wdiv"),
        rig.get16("wmod"),
        rig.get16("tmul"),
        rig.get16("tdiv"),
        rig.get16("tmod"),
        rig.get16("wshl"),
        rig.get16("wshr"),
        rig.get16("tsar"),
    ];
    let (div, rem) = udiv16(a, b);
    let (sdiv, srem) = sdiv16(ta, tb);
    let want = [
        a.wrapping_mul(b),
        div,
        rem,
        ta.wrapping_mul(tb) as u16,
        sdiv as u16,
        srem as u16,
        shl(a, count, 16),
        shr(a, count, 16),
        sar(ta, count) as u16,
    ];
    assert_eq!(
        got, want,
        "{a} and {b} ({ta} and {tb}), count {count}: * / % signed * / % << >> signed >>"
    );
}

#[test]
fn every_pair_of_bytes() {
    let mut rig = Rig::new();
    for a in 0..=255u8 {
        for b in 0..=255u8 {
            rig.run(a, b, 0, 0);
            check8(&rig, a, b);
        }
    }
}

#[test]
fn sixteen_bit_pairs() {
    let mut rig = Rig::new();
    let special: [u16; 30] = [
        0, 1, 2, 3, 7, 10, 15, 16, 100, 127, 128, 129, 255, 256, 257, 1000, 0x7ffe, 0x7fff, 0x8000,
        0x8001, 0xff00, 0xff7f, 0xff80, 0xff81, 0xfffd, 0xfffe, 0xffff, 0x00ff, 0x0f0f, 0xaaaa,
    ];
    for (i, &a) in special.iter().enumerate() {
        for (j, &b) in special.iter().enumerate() {
            let count = (i * 30 + j) as u8 % 20;
            rig.run(0, count, a, b);
            check16(&rig, a, b, count);
        }
    }
    // xorshift: the same pairs every time.
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    for _ in 0..20_000 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let (a, mut b) = (seed as u16, (seed >> 16) as u16);
        // Small divisors give large quotients; make a third of them small.
        match (seed >> 32) % 3 {
            0 => b >>= 8,
            1 => b = (b as i16 >> 9) as u16,
            _ => {}
        }
        let count = (seed >> 40) as u8 % 20;
        rig.run(0, count, a, b);
        check16(&rig, a, b, count);
    }
}
