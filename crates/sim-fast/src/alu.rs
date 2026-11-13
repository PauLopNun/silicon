//! Arithmetic of the Silicon CPU as plain functions: value in, value and
//! flags out. These are the reference the gate-level ALU is tested against.

use silicon_isa::flag::{C, N, V, Z};
use silicon_isa::AluOp;

/// `Z` and `N` for a result.
#[inline]
fn zn(result: u8) -> u8 {
    (if result == 0 { Z } else { 0 }) | (if result & 0x80 != 0 { N } else { 0 })
}

/// `x + y + carry_in` exactly as the 8-bit adder computes it, with all four
/// flags. Subtraction is the same adder fed with `!y`.
#[inline]
pub fn add(x: u8, y: u8, carry_in: bool) -> (u8, u8) {
    let wide = x as u16 + y as u16 + carry_in as u16;
    let result = wide as u8;
    let mut flags = zn(result);
    if wide > 0xFF {
        flags |= C;
    }
    // Overflow: both inputs have the same sign and the result has the other.
    if (x ^ result) & (y ^ result) & 0x80 != 0 {
        flags |= V;
    }
    (result, flags)
}

/// `adc` and `sbc` can clear `Z` but never set it, so that after a
/// multi-byte operation `Z` describes the whole value.
#[inline]
fn sticky_z((result, flags): (u8, u8), old_flags: u8) -> (u8, u8) {
    (result, flags & (old_flags | !Z))
}

/// The eight operations of the ALU instruction group (`op a, …`). Returns the
/// new value of `a` and the new flags.
#[inline]
pub fn alu(op: AluOp, x: u8, y: u8, flags: u8) -> (u8, u8) {
    let carry = flags & C != 0;
    // Logic operations change only Z and N.
    let logic = |result: u8| (result, (flags & (C | V)) | zn(result));
    match op {
        AluOp::Add => add(x, y, false),
        AluOp::Adc => sticky_z(add(x, y, carry), flags),
        AluOp::Sub => add(x, !y, true),
        AluOp::Sbc => sticky_z(add(x, !y, carry), flags),
        AluOp::And => logic(x & y),
        AluOp::Or => logic(x | y),
        AluOp::Xor => logic(x ^ y),
        AluOp::Cmp => (x, add(x, !y, true).1),
    }
}

/// `inc r`: `C` is left alone so that counters can sit inside carry chains.
#[inline]
pub fn inc(value: u8, flags: u8) -> (u8, u8) {
    let (result, f) = add(0x00, value, true);
    (result, (flags & C) | (f & (Z | N | V)))
}

/// `dec r`: adds `0xFF`; `C` is left alone.
#[inline]
pub fn dec(value: u8, flags: u8) -> (u8, u8) {
    let (result, f) = add(0xFF, value, false);
    (result, (flags & C) | (f & (Z | N | V)))
}

/// `shr` / `ror` / `asr`: shift right by one, `fill` enters at bit 7 and
/// bit 0 leaves into `C`. `V` is left alone.
#[inline]
pub fn shift_right(value: u8, fill: bool, flags: u8) -> (u8, u8) {
    let result = (value >> 1) | (fill as u8) << 7;
    let carry = if value & 1 != 0 { C } else { 0 };
    (result, (flags & V) | zn(result) | carry)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The adder against Rust's own checked arithmetic, for every input.
    #[test]
    fn add_matches_integer_arithmetic_exhaustively() {
        for x in 0..=255u8 {
            for y in 0..=255u8 {
                for carry in [false, true] {
                    let (r, f) = add(x, y, carry);
                    let unsigned = x as u32 + y as u32 + carry as u32;
                    let signed = x as i8 as i32 + y as i8 as i32 + carry as i32;
                    assert_eq!(r, unsigned as u8);
                    assert_eq!(f & C != 0, unsigned > 255);
                    assert_eq!(f & V != 0, !(-128..=127).contains(&signed));
                    assert_eq!(f & Z != 0, r == 0);
                    assert_eq!(f & N != 0, r >= 0x80);
                }
            }
        }
    }

    #[test]
    fn sub_and_cmp_follow_the_no_borrow_convention() {
        for x in 0..=255u8 {
            for y in 0..=255u8 {
                let (r, f) = alu(AluOp::Sub, x, y, 0);
                assert_eq!(r, x.wrapping_sub(y));
                assert_eq!(f & C != 0, x >= y, "C means no borrow");
                assert_eq!(f & Z != 0, x == y);
                // Signed comparison is N != V.
                assert_eq!((f & N != 0) != (f & V != 0), (x as i8) < (y as i8));
                // cmp is sub without the result.
                assert_eq!(alu(AluOp::Cmp, x, y, 0), (x, f));
            }
        }
    }

    #[test]
    fn adc_and_sbc_chain_into_16_bit_arithmetic() {
        let cases = [
            (0x0000u16, 0x0000u16),
            (0x00FF, 0x0001),
            (0x1234, 0xEDCC),
            (0xFFFF, 0x0001),
            (0x8000, 0x8000),
            (0x7FFF, 0x0001),
            (0x0100, 0x0100),
            (0x0001, 0x0100),
        ];
        for (x, y) in cases {
            let [xl, xh] = x.to_le_bytes();
            let [yl, yh] = y.to_le_bytes();

            let (lo, f) = alu(AluOp::Add, xl, yl, 0);
            let (hi, f) = alu(AluOp::Adc, xh, yh, f);
            let sum = x.wrapping_add(y);
            assert_eq!(u16::from_le_bytes([lo, hi]), sum);
            assert_eq!(f & Z != 0, sum == 0, "Z covers both bytes of {x:#x}+{y:#x}");
            assert_eq!(f & C != 0, x as u32 + y as u32 > 0xFFFF);

            let (lo, f) = alu(AluOp::Sub, xl, yl, 0);
            let (hi, f) = alu(AluOp::Sbc, xh, yh, f);
            assert_eq!(u16::from_le_bytes([lo, hi]), x.wrapping_sub(y));
            assert_eq!(f & Z != 0, x == y);
            assert_eq!(f & C != 0, x >= y);
            assert_eq!((f & N != 0) != (f & V != 0), (x as i16) < (y as i16));
        }
    }

    #[test]
    fn logic_operations_keep_carry_and_overflow() {
        for keep in [0, C, V, C | V] {
            assert_eq!(alu(AluOp::And, 0xF0, 0x0F, keep | N), (0x00, keep | Z));
            assert_eq!(alu(AluOp::Or, 0xF0, 0x0F, keep | Z), (0xFF, keep | N));
            assert_eq!(alu(AluOp::Xor, 0xFF, 0x0F, keep), (0xF0, keep | N));
        }
    }

    #[test]
    fn inc_and_dec_wrap_and_preserve_carry() {
        assert_eq!(inc(0xFF, C), (0x00, C | Z));
        assert_eq!(inc(0xFF, 0), (0x00, Z));
        assert_eq!(inc(0x7F, 0), (0x80, N | V));
        assert_eq!(dec(0x00, 0), (0xFF, N));
        assert_eq!(dec(0x01, C), (0x00, C | Z));
        assert_eq!(dec(0x80, 0), (0x7F, V));
        for v in 0..=255u8 {
            assert_eq!(inc(v, 0).0, v.wrapping_add(1));
            assert_eq!(dec(v, 0).0, v.wrapping_sub(1));
        }
    }

    #[test]
    fn right_shifts() {
        assert_eq!(shift_right(0b1000_0001, false, 0), (0b0100_0000, C));
        assert_eq!(shift_right(0b0000_0001, false, V), (0, V | Z | C));
        assert_eq!(shift_right(0b0000_0010, true, 0), (0b1000_0001, N));
        for v in 0..=255u8 {
            // asr is a signed division by two, rounding down.
            assert_eq!(shift_right(v, v & 0x80 != 0, 0).0, ((v as i8) >> 1) as u8);
        }
    }

    #[test]
    fn shifting_left_is_adding_a_number_to_itself() {
        for v in 0..=255u8 {
            let (r, f) = alu(AluOp::Add, v, v, 0);
            assert_eq!(r, v << 1);
            assert_eq!(f & C != 0, v & 0x80 != 0);
            // rol: adc a, a shifts the old carry into bit 0.
            assert_eq!(alu(AluOp::Adc, v, v, C).0, (v << 1) | 1);
        }
    }
}
