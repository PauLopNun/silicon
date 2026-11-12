//! Small example circuits used by tests and by the M0 walking skeleton.
//! The real component library (adders, muxes, registers…) arrives in M1.

use crate::{Builder, Netlist};

/// XOR out of four NAND gates: inputs `a`, `b`, output `y`.
///
/// ```text
///   n0 = nand(a, b)
///   n1 = nand(a, n0)     n2 = nand(b, n0)
///   y  = nand(n1, n2)
/// ```
pub fn xor() -> Netlist {
    let mut b = Builder::new("demo");
    let a = b.input("a");
    let bb = b.input("b");
    let y = b.scope("xor", |b| {
        let n0 = b.nand(a, bb);
        let n1 = b.nand(a, n0);
        let n2 = b.nand(bb, n0);
        b.nand(n1, n2)
    });
    b.output("y", y);
    b.finish().expect("xor demo is a valid netlist")
}
