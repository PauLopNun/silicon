//! The renderer of maze3d as plain Rust: what `programs/maze3d/render.asm`
//! computes, step for step, with ordinary integers and the same tables. The
//! tests of the renderer compare the program with this, and this with
//! floating-point geometry; the tests of the game use it to know what the
//! screen must show from wherever the player has walked to.

use silicon_machine::board::map;
use silicon_programs::tables;

/// Where the player is and where they look.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// Position in cells, 8.8 fixed point.
    pub x: u16,
    pub y: u16,
    /// 256 to a full turn; 0 is east, 64 is south.
    pub angle: u8,
}

/// Half the height of the wall in each column, and whether the ray met a
/// face that looks east or west (the brighter ones).
pub type Walls = Vec<(u8, bool)>;

/// What the program computes, step for step, with ordinary integers.
pub fn model(maze: &[u8], view: View) -> Walls {
    let (col_angle, inv_cos) = (tables::col_angle(), tables::inv_cos());
    let (log_near, log_far) = (tables::log_near(), tables::log_far());
    let (log_cos, height) = (tables::log_cos(), tables::height());
    let cell = (view.y >> 8) as usize * 16 + (view.x >> 8) as usize;
    let (frac_x, frac_y) = (view.x & 0xFF, view.y & 0xFF);

    (0..tables::COLUMNS)
        .map(|c| {
            // Direction of this ray, in 1024ths of a turn.
            let direction = (view.angle as usize * 4 + col_angle[c] as usize + 1024 - 128) % 1024;
            let (quadrant, inside) = (direction >> 8, direction & 0xFF);
            // In odd quadrants the angle is measured from the other axis.
            let along_x = if quadrant % 2 == 0 {
                inside
            } else {
                255 - inside
            };
            let (dx, dy) = (inv_cos[along_x] as u32, inv_cos[255 - along_x] as u32);
            // Moving towards higher x in quadrants 0 and 3, higher y in 0 and 1.
            let (east, south) = (quadrant == 0 || quadrant == 3, quadrant < 2);
            let to_line_x = if east { 255 - frac_x } else { frac_x } as u32;
            let to_line_y = if south { 255 - frac_y } else { frac_y } as u32;
            let mut ray_x = (to_line_x * dx) >> 8;
            let mut ray_y = (to_line_y * dy) >> 8;

            let (mut column, mut row) = ((cell % 16) as i32, (cell / 16) as i32);
            let (length, bright) = loop {
                if ray_x < ray_y {
                    column += if east { 1 } else { -1 };
                    if maze[(row * 16 + column) as usize] != 0 {
                        break (ray_x, true);
                    }
                    ray_x += dx;
                } else {
                    row += if south { 1 } else { -1 };
                    if maze[(row * 16 + column) as usize] != 0 {
                        break (ray_y, false);
                    }
                    ray_y += dy;
                }
            };

            let log = if length < 4 << 8 {
                log_near[(length >> 2) as usize]
            } else {
                log_far[(length >> 6) as usize]
            };
            (height[log.saturating_sub(log_cos[c]) as usize], bright)
        })
        .collect()
}

/// The screen for those walls: ceiling (shade 0), wall (2 or 3), floor (1).
pub fn paint(walls: &Walls) -> Vec<u8> {
    let mut screen = vec![0u8; map::FB_SIZE];
    for (c, &(half, bright)) in walls.iter().enumerate() {
        let half = half as usize;
        for y in 0..map::SCREEN_HEIGHT {
            let shade = if y < 32 - half {
                0
            } else if y < 32 + half {
                2 + bright as u8
            } else {
                1
            };
            screen[y * map::ROW_BYTES + c / 4] |= shade << (2 * (c % 4));
        }
    }
    screen
}
