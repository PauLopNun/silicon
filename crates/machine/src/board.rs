//! Everything on the bus that is not the CPU: ROM, RAM, the framebuffer and
//! the device registers. These are behavioural (ADR 0002) and shared by both
//! engines, so switching engines never copies memory.
//!
//! The layout is specified in `docs/ISA.md` §4. Two properties matter to the
//! rest of the project (ADR 0008):
//!
//! * **reads have no side effects**, so two boards given the same accesses
//!   stay identical and a debugger can look at any address;
//! * **time is counted in clock cycles**, never in wall-clock time, so a
//!   program behaves the same on both engines and in every test run.

use crate::Bus;
use std::fmt;

/// The memory map.
pub mod map {
    /// ROM: `0x0000`–`0x7FFF`.
    pub const ROM_SIZE: usize = 0x8000;
    /// Framebuffer: `0x8000`–`0x87FF`.
    pub const FB_START: u16 = 0x8000;
    pub const FB_SIZE: usize = 0x0800;
    /// RAM: `0x9000`–`0xFFFF`.
    pub const RAM_START: u16 = 0x9000;

    pub const SCREEN_WIDTH: usize = 128;
    pub const SCREEN_HEIGHT: usize = 64;
    /// Bytes per screen row: four 2-bit pixels per byte.
    pub const ROW_BYTES: usize = SCREEN_WIDTH / 4;

    /// R: keys currently held, one bit each (see [`super::key`]).
    pub const KEYS: u16 = 0x8800;
    /// W: the program has finished drawing a frame. R: frames finished so
    /// far, low byte.
    pub const FRAME: u16 = 0x8801;
    /// R: clock cycles since reset, bits 15..8. Ticks every 256 cycles.
    pub const TIMER_LO: u16 = 0x8802;
    /// R: clock cycles since reset, bits 23..16. Ticks every 65,536 cycles.
    pub const TIMER_HI: u16 = 0x8803;
    /// W: append a byte to the console.
    pub const OUT: u16 = 0x8804;
}

/// Bits of the [`map::KEYS`] register.
pub mod key {
    pub const UP: u8 = 1 << 0;
    pub const DOWN: u8 = 1 << 1;
    pub const LEFT: u8 = 1 << 2;
    pub const RIGHT: u8 = 1 << 3;
    pub const A: u8 = 1 << 4;
    pub const B: u8 = 1 << 5;
    pub const START: u8 = 1 << 6;
    pub const SELECT: u8 = 1 << 7;
}

/// A program image larger than the ROM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RomTooLarge {
    pub bytes: usize,
}

impl fmt::Display for RomTooLarge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "program is {} bytes but the ROM holds {}",
            self.bytes,
            map::ROM_SIZE
        )
    }
}

impl std::error::Error for RomTooLarge {}

/// Memory and devices.
#[derive(Clone, PartialEq, Eq)]
pub struct Board {
    /// The whole address space as one array, so that a memory access is an
    /// index. The device page and the unused gap are never written and stay 0.
    mem: Vec<u8>,
    keys: u8,
    frames: u64,
    cycles: u64,
    console: Vec<u8>,
}

/// 64 KiB of memory is not worth printing: summarise it.
impl fmt::Debug for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Board")
            .field("keys", &self.keys)
            .field("frames", &self.frames)
            .field("cycles", &self.cycles)
            .field("console", &String::from_utf8_lossy(&self.console))
            .field(
                "nonzero_bytes",
                &self.mem.iter().filter(|&&b| b != 0).count(),
            )
            .finish()
    }
}

impl Board {
    /// A board whose ROM holds `program`, starting at address 0. The rest of
    /// the ROM, the RAM and the framebuffer are zero.
    pub fn new(program: &[u8]) -> Result<Self, RomTooLarge> {
        if program.len() > map::ROM_SIZE {
            return Err(RomTooLarge {
                bytes: program.len(),
            });
        }
        let mut mem = vec![0; 0x1_0000];
        mem[..program.len()].copy_from_slice(program);
        Ok(Board {
            mem,
            keys: 0,
            frames: 0,
            cycles: 0,
            console: Vec::new(),
        })
    }

    /// Power-cycle: RAM, framebuffer and device state back to zero. The ROM
    /// and the keys being held are kept.
    pub fn reset(&mut self) {
        self.mem[map::ROM_SIZE..].fill(0);
        self.frames = 0;
        self.cycles = 0;
        self.console.clear();
    }

    pub fn rom(&self) -> &[u8] {
        &self.mem[..map::ROM_SIZE]
    }

    pub fn ram(&self) -> &[u8] {
        &self.mem[map::RAM_START as usize..]
    }

    /// The 2048 bytes of the screen, row by row.
    pub fn framebuffer(&self) -> &[u8] {
        let start = map::FB_START as usize;
        &self.mem[start..start + map::FB_SIZE]
    }

    /// Shade (0 = darkest … 3 = brightest) of the pixel at column `x`, row
    /// `y`. Pixel 0 of a byte is its two least significant bits.
    pub fn pixel(&self, x: usize, y: usize) -> u8 {
        let byte = self.framebuffer()[y * map::ROW_BYTES + x / 4];
        byte >> (2 * (x % 4)) & 3
    }

    /// Any byte of the address space, as a read by the CPU would see it.
    pub fn peek(&self, addr: u16) -> u8 {
        if addr >> 8 == map::KEYS >> 8 {
            self.register(addr)
        } else {
            self.mem[addr as usize]
        }
    }

    /// Press and release keys: `keys` is the new value of the key register.
    pub fn set_keys(&mut self, keys: u8) {
        self.keys = keys;
    }

    pub fn keys(&self) -> u8 {
        self.keys
    }

    /// Frames the program has declared finished since reset.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Clock cycles since reset.
    pub fn cycles(&self) -> u64 {
        self.cycles
    }

    /// Everything the program has written to the console since reset (or
    /// since the last [`Board::take_console`]).
    pub fn console(&self) -> &[u8] {
        &self.console
    }

    /// Remove and return the console output.
    pub fn take_console(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.console)
    }

    fn register(&self, addr: u16) -> u8 {
        match addr {
            map::KEYS => self.keys,
            map::FRAME => self.frames as u8,
            map::TIMER_LO => (self.cycles >> 8) as u8,
            map::TIMER_HI => (self.cycles >> 16) as u8,
            _ => 0,
        }
    }
}

impl Bus for Board {
    #[inline]
    fn read(&mut self, addr: u16) -> u8 {
        self.peek(addr)
    }

    #[inline]
    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            // Framebuffer and RAM.
            0x8000..=0x87FF | 0x9000..=0xFFFF => self.mem[addr as usize] = value,
            map::FRAME => self.frames += 1,
            map::OUT => self.console.push(value),
            // ROM, read-only registers and the unused gap ignore writes.
            _ => {}
        }
    }

    #[inline]
    fn tick(&mut self) {
        self.cycles += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rom_is_read_only_and_holds_the_program() {
        let mut board = Board::new(&[1, 2, 3]).unwrap();
        assert_eq!((board.read(0), board.read(2), board.read(3)), (1, 3, 0));
        board.write(0x0001, 0xFF);
        board.write(0x7FFF, 0xFF);
        assert_eq!((board.read(0x0001), board.read(0x7FFF)), (2, 0));
        assert_eq!(board.rom().len(), 0x8000);
    }

    #[test]
    fn a_program_must_fit_in_the_rom() {
        assert!(Board::new(&vec![0; 0x8000]).is_ok());
        let err = Board::new(&vec![0; 0x8001]).err().unwrap();
        assert_eq!(err, RomTooLarge { bytes: 0x8001 });
        assert_eq!(
            err.to_string(),
            "program is 32769 bytes but the ROM holds 32768"
        );
    }

    #[test]
    fn ram_and_framebuffer_are_read_write() {
        let mut board = Board::new(&[]).unwrap();
        for addr in [0x8000u16, 0x87FF, 0x9000, 0xFFFF] {
            board.write(addr, 0x5A);
            assert_eq!(board.read(addr), 0x5A, "{addr:#06x}");
        }
        assert_eq!(board.framebuffer()[0], 0x5A);
        assert_eq!(board.framebuffer().len(), 2048);
        assert_eq!(board.ram()[0], 0x5A);
        assert_eq!(board.ram().len(), 0x7000);
    }

    #[test]
    fn the_gap_and_unused_registers_read_zero_and_ignore_writes() {
        let mut board = Board::new(&[]).unwrap();
        for addr in [0x8805u16, 0x88FF, 0x8900, 0x8FFF, map::KEYS, map::TIMER_LO] {
            board.write(addr, 0xFF);
            assert_eq!(board.read(addr), 0, "{addr:#06x}");
        }
        assert_eq!(board, Board::new(&[]).unwrap(), "nothing changed");
    }

    #[test]
    fn pixels_are_two_bits_least_significant_first() {
        let mut board = Board::new(&[]).unwrap();
        // 0b11_10_01_00: pixels 0..3 of the byte have shades 0, 1, 2, 3.
        board.write(map::FB_START, 0b1110_0100);
        board.write(map::FB_START + 32 + 1, 0b0000_0011);
        assert_eq!(
            [0, 1, 2, 3].map(|x| board.pixel(x, 0)),
            [0, 1, 2, 3],
            "first byte of the first row"
        );
        assert_eq!(board.pixel(4, 1), 3, "second row, second byte");
        assert_eq!(board.pixel(5, 1), 0);
    }

    #[test]
    fn keys_frames_and_console() {
        let mut board = Board::new(&[]).unwrap();
        board.set_keys(key::LEFT | key::A);
        assert_eq!(board.read(map::KEYS), 0b0001_0100);
        for _ in 0..300 {
            board.write(map::FRAME, 0);
        }
        assert_eq!(board.frames(), 300);
        assert_eq!(board.read(map::FRAME), 300u16 as u8, "low byte");
        board.write(map::OUT, b'h');
        board.write(map::OUT, b'i');
        assert_eq!(board.console(), b"hi");
        assert_eq!(board.take_console(), b"hi");
        assert!(board.console().is_empty());
    }

    #[test]
    fn the_timer_counts_clock_cycles() {
        let mut board = Board::new(&[]).unwrap();
        for _ in 0..255 {
            board.tick();
        }
        assert_eq!(board.read(map::TIMER_LO), 0);
        board.tick();
        assert_eq!(
            (board.read(map::TIMER_LO), board.read(map::TIMER_HI)),
            (1, 0)
        );
        for _ in 256..0x1_0000 + 0x0300 {
            board.tick();
        }
        assert_eq!(
            (board.read(map::TIMER_LO), board.read(map::TIMER_HI)),
            (3, 1)
        );
        assert_eq!(board.cycles(), 0x1_0300);
    }

    #[test]
    fn reads_never_change_anything() {
        let mut board = Board::new(&[9, 9]).unwrap();
        board.set_keys(key::UP);
        board.write(map::FRAME, 0);
        board.write(map::OUT, b'x');
        let before = board.clone();
        for addr in 0..=0xFFFFu16 {
            assert_eq!(board.read(addr), before.peek(addr));
        }
        assert!(board == before);
    }

    #[test]
    fn reset_keeps_the_rom_and_the_keys() {
        let mut board = Board::new(&[7]).unwrap();
        board.set_keys(key::B);
        board.write(0x9000, 1);
        board.write(map::FB_START, 1);
        board.write(map::FRAME, 0);
        board.write(map::OUT, b'x');
        board.tick();
        board.reset();
        let mut fresh = Board::new(&[7]).unwrap();
        fresh.set_keys(key::B);
        assert!(board == fresh);
    }
}
