//! The lookup tables of `maze3d`, computed here and checked in as
//! `programs/maze3d/tables.inc`. A test keeps that file identical to this
//! generator; `SILICON_BLESS=1 cargo test -p silicon-programs` rewrites it.
//!
//! The mathematics uses only `+ − × ÷` and square roots, which IEEE 754
//! defines exactly, so the tables come out the same on every machine. (The
//! library's `sin`, `atan` and `log2` may differ in the last bit from one
//! platform to the next.)

use std::f64::consts::{LN_2, PI};
use std::fmt::Write;

/// Screen width in pixels: one ray per column.
pub const COLUMNS: usize = 128;
/// Distance from the eye to the screen, in pixels. With 128 columns this is
/// a field of view of 83°.
pub const FOCAL: f64 = 72.0;
/// Half the height in pixels of a wall one cell away. Half of [`FOCAL`]:
/// walls are as tall as cells are wide.
pub const WALL: f64 = 36.0;
/// Half the screen height: a wall cannot be drawn taller.
pub const MAX_HALF_HEIGHT: f64 = 32.0;
/// Units of the logarithm tables per doubling of distance.
pub const LOG_SCALE: f64 = 64.0;
/// Directions in a full turn. A quadrant is 256 of them, so the position
/// inside a quadrant is one byte.
pub const TURN: f64 = 1024.0;
/// `inv_cos` is capped here (224 cells) so that adding it to a distance
/// inside the map can never overflow 16 bits.
pub const INV_COS_MAX: u16 = 0xE000;

/// cos x for 0 ≤ x ≤ π/2, by its Taylor series.
fn cos(x: f64) -> f64 {
    let (mut term, mut sum) = (1.0, 1.0);
    for k in 1..=16 {
        term *= -x * x / ((2 * k - 1) as f64 * (2 * k) as f64);
        sum += term;
    }
    sum
}

/// atan x for |x| ≤ 1. Halving the angle twice makes the series converge
/// quickly: atan x = 2 atan(x / (1 + √(1 + x²))).
fn atan(x: f64) -> f64 {
    let half = |x: f64| x / (1.0 + (1.0 + x * x).sqrt());
    let t = half(half(x));
    let (mut power, mut sum) = (t, t);
    for k in 1..=40 {
        power *= -t * t;
        sum += power / (2 * k + 1) as f64;
    }
    4.0 * sum
}

/// log₂ x for x > 0: take out the powers of two, then
/// ln m = 2 atanh((m − 1)/(m + 1)).
fn log2(x: f64) -> f64 {
    let (mut m, mut exponent) = (x, 0.0);
    while m >= 2.0 {
        m /= 2.0;
        exponent += 1.0;
    }
    while m < 1.0 {
        m *= 2.0;
        exponent -= 1.0;
    }
    let t = (m - 1.0) / (m + 1.0);
    let (mut power, mut sum) = (t, t);
    for k in 1..=40 {
        power *= t * t;
        sum += power / (2 * k + 1) as f64;
    }
    exponent + 2.0 * sum / LN_2
}

/// 2ˣ for x ≥ 0.
fn exp2(x: f64) -> f64 {
    let whole = x.floor();
    let z = (x - whole) * LN_2;
    let (mut term, mut sum) = (1.0, 1.0);
    for k in 1..=30 {
        term *= z / k as f64;
        sum += term;
    }
    let mut result = sum;
    for _ in 0..whole as u32 {
        result *= 2.0;
    }
    result
}

/// Where column `c` is on the screen, in units of the focal length: the
/// tangent of the angle between its ray and the direction of view.
fn slope(c: usize) -> f64 {
    (c as f64 - (COLUMNS as f64 - 1.0) / 2.0) / FOCAL
}

/// `col_angle[c]`: the direction of the ray through column `c` relative to
/// the direction of view, plus 128 so that it is never negative.
pub fn col_angle() -> Vec<u8> {
    (0..COLUMNS)
        .map(|c| (atan(slope(c)) * TURN / (2.0 * PI)).round() as i32 + 128)
        .map(|v| u8::try_from(v).expect("the field of view fits in a byte"))
        .collect()
}

/// `inv_cos[i]` = 1 / cos(angle), as 8.8 fixed point, for the 256 angles of
/// a quadrant. They are taken at the middle of each step, so no ray is ever
/// exactly parallel to a grid line and the table read backwards is 1 / sin.
pub fn inv_cos() -> Vec<u16> {
    (0..256)
        .map(|i| {
            let angle = (i as f64 + 0.5) * (2.0 * PI / TURN);
            let value = (256.0 / cos(angle)).round();
            value.min(INV_COS_MAX as f64) as u16
        })
        .collect()
}

/// `squares[n]` = ⌊n² / 4⌋ for n = 0…511: multiplication without a
/// multiplier, because a × b = squares[a + b] − squares[|a − b|].
pub fn squares() -> Vec<u16> {
    (0..512u32).map(|n| (n * n / 4) as u16).collect()
}

fn log_of(distance: f64) -> u8 {
    if distance <= 1.0 {
        return 0;
    }
    (LOG_SCALE * log2(distance)).round().min(255.0) as u8
}

/// `log_near[i]`: logarithm of the distance `i / 64` cells (used below 4
/// cells, where a sixty-fourth of a cell changes the picture).
pub fn log_near() -> Vec<u8> {
    (0..256).map(|i| log_of(i as f64 / 64.0)).collect()
}

/// `log_far[i]`: logarithm of the distance `i / 4` cells.
pub fn log_far() -> Vec<u8> {
    (0..256).map(|i| log_of(i as f64 / 4.0)).collect()
}

/// `log_cos[c]`: what to subtract from the logarithm of a distance measured
/// along the ray of column `c` to get the distance measured straight ahead,
/// which is the one that decides how tall a wall looks. Without it, walls
/// would bulge towards the viewer in the middle of the screen.
///
/// 1 / cos² = 1 + tan², so it is half the logarithm of 1 + slope².
pub fn log_cos() -> Vec<u8> {
    (0..COLUMNS)
        .map(|c| (LOG_SCALE / 2.0 * log2(1.0 + slope(c) * slope(c))).round() as u8)
        .collect()
}

/// `height[l]`: half the height in pixels of a wall whose distance has
/// logarithm `l`.
pub fn height() -> Vec<u8> {
    (0..256)
        .map(|l| {
            let distance = exp2(l as f64 / LOG_SCALE);
            (WALL / distance).round().clamp(1.0, MAX_HALF_HEIGHT) as u8
        })
        .collect()
}

fn table(out: &mut String, name: &str, about: &str, values: &[u8]) {
    let _ = writeln!(out);
    for line in about.lines() {
        let _ = writeln!(out, "; {line}");
    }
    let _ = writeln!(out, "{name}:");
    for row in values.chunks(16) {
        let row: Vec<String> = row.iter().map(|v| format!("{v:3}")).collect();
        let _ = writeln!(out, "        .byte {}", row.join(","));
    }
}

/// The whole of `programs/maze3d/tables.inc`.
pub fn maze3d() -> String {
    let lo = |values: &[u16]| values.iter().map(|v| *v as u8).collect::<Vec<u8>>();
    let hi = |values: &[u16]| values.iter().map(|v| (*v >> 8) as u8).collect::<Vec<u8>>();
    let mut out = String::from(
        "; Lookup tables of maze3d.\n\
         ;\n\
         ; GENERATED by crates/programs/src/tables.rs, which explains each of them.\n\
         ; Do not edit: change the generator and run\n\
         ;     SILICON_BLESS=1 cargo test -p silicon-programs\n",
    );
    table(
        &mut out,
        "col_angle",
        "Direction of the ray through each screen column, relative to the\n\
         direction of view, in 1024ths of a turn, plus 128.",
        &col_angle(),
    );
    let inv = inv_cos();
    table(
        &mut out,
        "inv_cos_lo",
        "256 / cos(angle) for the 256 angles of a quadrant: how far a ray travels\n\
         to cross one cell. Low bytes, then high bytes.",
        &lo(&inv),
    );
    table(&mut out, "inv_cos_hi", "", &hi(&inv));
    let sq = squares();
    out.push_str("\n        .align 256\n");
    table(
        &mut out,
        "squares",
        "n * n / 4 for n = 0...511: low bytes (two pages), then high bytes (two\n\
         pages). a * b = squares[a + b] - squares[|a - b|].",
        &lo(&sq),
    );
    table(&mut out, "squares_hi", "", &hi(&sq));
    table(
        &mut out,
        "log_near",
        "64 * log2(distance) for distance = index / 64 cells.",
        &log_near(),
    );
    table(
        &mut out,
        "log_far",
        "64 * log2(distance) for distance = index / 4 cells.",
        &log_far(),
    );
    table(
        &mut out,
        "log_cos",
        "For each screen column, 64 * log2 of (distance along its ray / distance\n\
         straight ahead).",
        &log_cos(),
    );
    table(
        &mut out,
        "height",
        "Half the height in pixels of a wall, by the logarithm of its distance.",
        &height(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The series agree with the standard library far beyond the precision
    /// any table needs.
    #[test]
    fn the_series_are_accurate() {
        for i in 0..=1000 {
            let x = i as f64 / 1000.0;
            assert!(
                (cos(x * PI / 2.0) - (x * PI / 2.0).cos()).abs() < 1e-13,
                "cos {x}"
            );
            assert!((atan(x) - x.atan()).abs() < 1e-13, "atan {x}");
            assert!((atan(-x) + x.atan()).abs() < 1e-13, "atan -{x}");
            let y = 0.01 + x * 300.0;
            assert!((log2(y) - y.log2()).abs() < 1e-12, "log2 {y}");
            let z = x * 8.0;
            assert!((exp2(z) / z.exp2() - 1.0).abs() < 1e-13, "exp2 {z}");
        }
    }

    #[test]
    fn the_view_is_symmetric_and_fits_in_a_quadrant() {
        let angles = col_angle();
        assert_eq!(angles.len(), 128);
        for c in 0..64 {
            // Mirror columns look as far to the left as to the right.
            assert_eq!(angles[c] as u32 + angles[127 - c] as u32, 256, "column {c}");
        }
        // Every column has its own ray.
        assert!(angles.windows(2).all(|w| w[0] < w[1]));
        assert_eq!((angles[0], angles[127]), (10, 246));
        let correction = log_cos();
        assert_eq!(correction[63], 0, "straight ahead needs no correction");
        assert_eq!(correction[0], correction[127]);
        assert_eq!(correction[0], 27);
    }

    #[test]
    fn inverse_cosines_run_from_one_to_the_cap() {
        let inv = inv_cos();
        assert_eq!(
            inv[0], 256,
            "along an axis a ray crosses a cell in one cell"
        );
        assert_eq!(inv[255], INV_COS_MAX, "almost parallel: capped");
        assert_eq!(inv[254], (256.0 / (1.5 * PI / 512.0).sin()).round() as u16);
        assert!(inv.windows(2).all(|w| w[0] <= w[1]));
        // 45°: √2.
        assert!((inv[127] as f64 / 256.0 - 2f64.sqrt()).abs() < 0.01);
    }

    #[test]
    fn squares_multiply() {
        let sq = squares();
        for a in 0..256usize {
            for b in 0..256usize {
                assert_eq!((sq[a + b] - sq[a.abs_diff(b)]) as usize, a * b);
            }
        }
    }

    #[test]
    fn heights_follow_one_over_distance() {
        let (near, far, height) = (log_near(), log_far(), height());
        // Closer than one cell, a wall fills the screen.
        assert!(near[..=64].iter().all(|&l| l == 0));
        assert_eq!(height[0], 32);
        // The two distance tables meet at four cells.
        assert_eq!(near[255], 128);
        assert_eq!(far[16], 128);
        for (cells, pixels) in [
            (2.0, 18),
            (3.0, 12),
            (4.0, 9),
            (6.0, 6),
            (9.0, 4),
            (12.0, 3),
        ] {
            let l = if cells < 4.0 {
                near[(cells * 64.0) as usize]
            } else {
                far[(cells * 4.0) as usize]
            };
            assert_eq!(height[l as usize], pixels, "{cells} cells away");
        }
        assert!(height.windows(2).all(|w| w[0] >= w[1]));
        assert_eq!(height[255], 2, "the farthest wall is still visible");
    }
}
