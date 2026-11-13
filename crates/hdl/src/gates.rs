//! The familiar logic gates, each spelled out in NANDs.
//!
//! These are methods on [`Builder`] that place their NANDs in the current
//! module without opening a scope of their own: an `xor` is four gates of
//! whatever block uses it, not a block in itself.
//!
//! | Gate | NANDs | Depth |
//! |---|---|---|
//! | `not` | 1 | 1 |
//! | `and` | 2 | 2 |
//! | `or`  | 3 | 2 |
//! | `xor` | 4 | 3 |
//! | `mux` | 3 (+1 to invert the select, shared by a whole word) | 2 |

use crate::{Builder, Wire};

impl Builder {
    /// `!a`: a NAND with both inputs tied together.
    pub fn not(&mut self, a: Wire) -> Wire {
        self.nand(a, a)
    }

    /// `a & b`: a NAND followed by an inverter.
    pub fn and(&mut self, a: Wire, b: Wire) -> Wire {
        let n = self.nand(a, b);
        self.not(n)
    }

    /// `a | b`: by De Morgan, `!(!a & !b)`.
    pub fn or(&mut self, a: Wire, b: Wire) -> Wire {
        let na = self.not(a);
        let nb = self.not(b);
        self.nand(na, nb)
    }

    /// `!(a | b)`.
    pub fn nor(&mut self, a: Wire, b: Wire) -> Wire {
        let o = self.or(a, b);
        self.not(o)
    }

    /// `a ^ b` in four NANDs.
    pub fn xor(&mut self, a: Wire, b: Wire) -> Wire {
        let n = self.nand(a, b);
        let l = self.nand(a, n);
        let r = self.nand(b, n);
        self.nand(l, r)
    }

    /// `sel ? a1 : a0`.
    pub fn mux(&mut self, sel: Wire, a0: Wire, a1: Wire) -> Wire {
        let nsel = self.not(sel);
        self.mux_with(sel, nsel, a0, a1)
    }

    /// `sel ? a1 : a0`, given `nsel = !sel` computed once by the caller.
    pub fn mux_with(&mut self, sel: Wire, nsel: Wire, a0: Wire, a1: Wire) -> Wire {
        let t1 = self.nand(a1, sel);
        let t0 = self.nand(a0, nsel);
        self.nand(t0, t1)
    }

    /// AND of any number of wires, as a balanced tree. Empty input is `1`.
    pub fn and_all(&mut self, wires: &[Wire]) -> Wire {
        match wires {
            [] => Wire::ONE,
            [w] => *w,
            _ => {
                let n = self.nand_all(wires);
                self.not(n)
            }
        }
    }

    /// NAND of any number of wires: `!(w0 & w1 & …)`. Empty input is `0`.
    ///
    /// Because a NAND of active-low signals is the OR of the signals, this is
    /// also how the control unit sums its product terms.
    pub fn nand_all(&mut self, wires: &[Wire]) -> Wire {
        match wires {
            [] => Wire::ZERO,
            [w] => self.not(*w),
            _ => {
                let (left, right) = wires.split_at(wires.len().div_ceil(2));
                let l = self.and_all(left);
                let r = self.and_all(right);
                self.nand(l, r)
            }
        }
    }

    /// OR of any number of wires. Empty input is `0`.
    pub fn or_all(&mut self, wires: &[Wire]) -> Wire {
        match wires {
            [] => Wire::ZERO,
            [w] => *w,
            _ => {
                let inverted: Vec<Wire> = wires.iter().map(|&w| self.not(w)).collect();
                self.nand_all(&inverted)
            }
        }
    }
}

/// Plain Rust meaning of each gate, used as the test reference.
pub mod reference {
    pub fn mux(sel: bool, a0: bool, a1: bool) -> bool {
        if sel {
            a1
        } else {
            a0
        }
    }
}
