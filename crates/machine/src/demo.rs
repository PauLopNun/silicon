//! A small program for the computer, built in Rust because the assembler does
//! not exist yet (M3). It is what the web page runs until then.

use crate::board::{key, map};
use silicon_isa::{AluOp, Cond, Insn, Pair, Program, Reg};

/// Where the program keeps how far the stripes have scrolled.
const OFFSET: u16 = map::RAM_START;

/// The eight shades one period of the pattern goes through. Each byte is
/// four pixels of the same shade.
const SHADES: [u8; 8] = [0x00, 0x55, 0xAA, 0xFF, 0xFF, 0xAA, 0x55, 0x00];

/// Diagonal stripes that scroll one step per timer tick; holding **left**
/// reverses them.
///
/// It uses the whole machine: a table in ROM, a variable in RAM, the
/// framebuffer, the keys, the frame marker and the timer.
///
/// ```text
///         ld   y, shades
/// frame:  ld   x, 0x8000          ; framebuffer
///         ld   zh, 64             ; rows left
/// row:    ld   a, [offset]
///         add  a, zh
///         mov  zl, a              ; phase of the first byte of this row
///         ld   b, 32              ; bytes left in the row
/// byte:   mov  a, zl
///         and  a, 7
///         ld   a, [y+a]           ; shade from the table
///         st   [x+], a
///         inc  zl
///         dec  b
///         bnz  byte
///         dec  zh
///         bnz  row
///         st   [FRAME], a         ; frame finished
///         ld   a, [KEYS]
///         and  a, LEFT
///         ld   a, [offset]        ; (loads leave the flags alone)
///         bnz  back
///         inc  a
///         jmp  store
/// back:   dec  a
/// store:  st   [offset], a
///         ld   a, [TIMER_HI]      ; wait for the next tick
///         mov  b, a
/// wait:   ld   a, [TIMER_HI]
///         cmp  a, b
///         bz   wait
///         jmp  frame
/// shades: .byte 0x00, 0x55, 0xaa, 0xff, 0xff, 0xaa, 0x55, 0x00
/// ```
pub fn stripes() -> Vec<u8> {
    use Insn::*;
    let mut p = Program::new();
    let (shades, back, store) = (p.label(), p.label(), p.label());

    p.ld_pair(Pair::Y, shades);
    let frame = p.here();
    p.push(LdPairImm(Pair::X, map::FB_START));
    p.push(LdImm(Reg::Zh, map::SCREEN_HEIGHT as u8));
    let row = p.here();
    p.extend(&[
        LdAAbs(OFFSET),
        Alu(AluOp::Add, Reg::Zh),
        Mov(Reg::Zl, Reg::A),
        LdImm(Reg::B, map::ROW_BYTES as u8),
    ]);
    let byte = p.here();
    p.extend(&[
        Mov(Reg::A, Reg::Zl),
        AluImm(AluOp::And, 7),
        LdAIdx(Pair::Y),
        StPtrIncA(Pair::X),
        Inc(Reg::Zl),
        Dec(Reg::B),
    ]);
    p.br(Cond::Nz, byte);
    p.push(Dec(Reg::Zh));
    p.br(Cond::Nz, row);

    p.extend(&[
        StAbsA(map::FRAME),
        LdAAbs(map::KEYS),
        AluImm(AluOp::And, key::LEFT),
        LdAAbs(OFFSET),
    ]);
    p.br(Cond::Nz, back);
    p.push(Inc(Reg::A));
    p.jmp(store);
    p.place(back);
    p.push(Dec(Reg::A));
    p.place(store);
    p.extend(&[StAbsA(OFFSET), LdAAbs(map::TIMER_HI), Mov(Reg::B, Reg::A)]);
    let wait = p.here();
    p.extend(&[LdAAbs(map::TIMER_HI), Alu(AluOp::Cmp, Reg::B)]);
    p.br(Cond::Z, wait);
    p.jmp(frame);

    p.place(shades);
    p.bytes(&SHADES);
    p.assemble().expect("the demo program assembles")
}

/// What the framebuffer must hold after [`stripes`] has drawn a frame with
/// the given scroll offset: the same picture, computed in plain Rust.
pub fn stripes_frame(offset: u8) -> Vec<u8> {
    let mut frame = Vec::with_capacity(map::FB_SIZE);
    for row in 0..map::SCREEN_HEIGHT {
        let rows_left = (map::SCREEN_HEIGHT - row) as u8;
        for column in 0..map::ROW_BYTES {
            let phase = offset.wrapping_add(rows_left).wrapping_add(column as u8);
            frame.push(SHADES[(phase & 7) as usize]);
        }
    }
    frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EngineKind, Machine};

    /// Three timer ticks, on each engine, with and without a key held: the
    /// screen must be exactly the reference picture.
    #[test]
    fn stripes_draws_the_reference_picture_on_both_engines() {
        for kind in [EngineKind::Fast, EngineKind::Gate] {
            for (keys, offset_shown) in [(0, 2u8), (key::LEFT, 254), (key::RIGHT | key::A, 2)] {
                let mut machine = Machine::new(&stripes()).unwrap();
                machine.set_engine(kind);
                machine.board_mut().set_keys(keys);
                // Frames are drawn at ticks 0, 1 and 2; stop before tick 3.
                machine.run(190_000);
                assert_eq!(machine.board().frames(), 3, "{kind:?}");
                assert!(
                    machine.board().framebuffer() == stripes_frame(offset_shown),
                    "{kind:?}, keys {keys:#04x}: wrong picture"
                );
                assert!(!machine.halted());
            }
        }
    }

    #[test]
    fn one_frame_fits_comfortably_in_a_timer_tick() {
        let mut machine = Machine::new(&stripes()).unwrap();
        let mut cycles = 0u64;
        while machine.board().frames() == 0 {
            machine.run(1);
            cycles += 1;
        }
        // 2,048 bytes at 12 cycles each, plus per-row overhead.
        assert!((24_000..28_000).contains(&cycles), "{cycles} cycles");
        assert!(machine.board().framebuffer() == stripes_frame(0));
    }

    #[test]
    fn the_reference_picture_is_diagonal_stripes() {
        let frame = stripes_frame(0);
        assert_eq!(frame.len(), 2048);
        // Moving one row down shifts the pattern one byte to the right.
        assert_eq!(frame[0..31], frame[33..64]);
        // Scrolling by a whole period gives the same picture.
        assert_eq!(stripes_frame(8), frame);
        assert_ne!(stripes_frame(1), frame);
    }
}
