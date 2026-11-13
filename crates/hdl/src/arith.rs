//! Adders. Each full adder is its own module (`fa0`, `fa1`…) of nine NANDs,
//! chained into a ripple-carry adder of any width.

use crate::{Builder, Wire};

/// Outputs of [`Builder::adder`].
#[derive(Clone, Debug)]
pub struct Sum {
    /// The sum, same width as the inputs.
    pub sum: Vec<Wire>,
    /// `carries[i]` is the carry out of bit `i`; the last one is the carry
    /// out of the whole adder.
    pub carries: Vec<Wire>,
}

impl Sum {
    /// Carry out of the most significant bit.
    pub fn carry_out(&self) -> Wire {
        *self.carries.last().expect("adder has at least one bit")
    }
}

impl Builder {
    /// One-bit full adder in nine NANDs, placed in the current module.
    /// Returns `(sum, carry_out)`.
    ///
    /// ```text
    ///   n0 = nand(a, b)
    ///   p  = a ^ b          = nand(nand(a, n0), nand(b, n0))
    ///   n3 = nand(p, cin)
    ///   s  = p ^ cin        = nand(nand(p, n3), nand(cin, n3))
    ///   cout = a&b | p&cin  = nand(n0, n3)
    /// ```
    pub fn full_adder(&mut self, a: Wire, b: Wire, cin: Wire) -> (Wire, Wire) {
        let n0 = self.nand(a, b);
        let n1 = self.nand(a, n0);
        let n2 = self.nand(b, n0);
        let p = self.nand(n1, n2);
        let n3 = self.nand(p, cin);
        let n4 = self.nand(p, n3);
        let n5 = self.nand(cin, n3);
        let sum = self.nand(n4, n5);
        let cout = self.nand(n0, n3);
        (sum, cout)
    }

    /// Ripple-carry adder: `x + y + cin`. Builds one child module per bit,
    /// `fa0` (least significant) upwards, inside the current module.
    pub fn adder(&mut self, x: &[Wire], y: &[Wire], cin: Wire) -> Sum {
        assert_eq!(x.len(), y.len(), "adder inputs differ in width");
        let mut carry = cin;
        let mut sum = Vec::with_capacity(x.len());
        let mut carries = Vec::with_capacity(x.len());
        for (i, (&a, &b)) in x.iter().zip(y).enumerate() {
            let (s, c) = self.scope(&format!("fa{i}"), |b_| b_.full_adder(a, b, carry));
            sum.push(s);
            carries.push(c);
            carry = c;
        }
        Sum { sum, carries }
    }
}

/// Plain Rust meaning of the adder: `(sum, carry_out)` for `n`-bit inputs.
pub fn add_reference(x: u64, y: u64, cin: bool, n: u32) -> (u64, bool) {
    let mask = (1u64 << n) - 1;
    let wide = (x & mask) + (y & mask) + cin as u64;
    (wide & mask, wide >> n != 0)
}
