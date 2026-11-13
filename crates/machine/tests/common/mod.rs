//! Helpers shared by the integration tests.
#![allow(dead_code)]

use silicon_isa::ArchState;

/// xorshift64*: a small deterministic generator, so failures reproduce.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn fill(&mut self, memory: &mut [u8]) {
        for chunk in memory.chunks_mut(8) {
            let bytes = self.next().to_le_bytes();
            chunk.copy_from_slice(&bytes[..chunk.len()]);
        }
    }

    /// Any state the CPU can be in, reachable from reset or not.
    pub fn state(&mut self) -> ArchState {
        let (a, b, c) = (self.next(), self.next(), self.next());
        ArchState {
            pc: a as u16,
            sp: (a >> 16) as u16,
            t: (a >> 32) as u16,
            ir: (a >> 48) as u8,
            flags: (a >> 56) as u8 & 0x0F,
            regs: b.to_le_bytes(),
            // Mostly at an instruction boundary or early in an instruction,
            // sometimes at a step the instruction does not even have.
            step: [0, 0, 1, 1, 1, 2, 2, 3, 4, 5, 6, 7][(c % 12) as usize],
            cycle: 0,
        }
    }
}
