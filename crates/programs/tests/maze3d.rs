//! The renderer of maze3d: is the picture right, and what does it cost?
//!
//! Three things are compared:
//!
//! 1. the screen the program draws, on both engines;
//! 2. `model`, the same arithmetic in plain Rust with the same tables,
//!    which must give that screen byte for byte;
//! 3. `geometry`, the textbook computation in floating point with no tables
//!    at all, which the model must match to within a pixel once the
//!    viewpoint is allowed to be as imprecise as 8-bit fractions make it.
//!
//! (1 = 2) says the assembly does what it was meant to; (2 ≈ 3) says that
//! what it was meant to do is to draw the maze.

mod common;

use common::{boot, build, picture, poke16, profile, run_frames, symbol};
use silicon_asm::Assembly;
use silicon_machine::board::map;
use silicon_machine::{Bus, EngineKind, Machine};
use silicon_programs::tables;
use std::f64::consts::PI;

/// Where the player is and where they look.
#[derive(Clone, Copy, Debug)]
struct View {
    /// Position in cells, 8.8 fixed point.
    x: u16,
    y: u16,
    /// 256 to a full turn; 0 is east, 64 is south.
    angle: u8,
}

/// Half the height of the wall in each column, and whether the ray met a
/// face that looks east or west (the brighter ones).
type Walls = Vec<(u8, bool)>;

/// The maze of the program, read from its ROM.
fn maze(program: &Assembly) -> Vec<u8> {
    let at = symbol(program, "_maze_map") as usize;
    program.rom[at..at + 256].to_vec()
}

// ───────────────────────── 2: the model ─────────────────────────

/// What the program computes, step for step, with ordinary integers.
fn model(maze: &[u8], view: View) -> Walls {
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
fn paint(walls: &Walls) -> Vec<u8> {
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

// ──────────────────────── 3: the geometry ────────────────────────

/// Half the height of the wall seen from (`x`, `y`) through screen column
/// `c` (which need not be whole), from first principles: follow the ray in
/// small steps of floating point until it is inside a wall.
fn geometry(maze: &[u8], x: f64, y: f64, angle: u8, c: f64) -> f64 {
    let across = (c - 63.5) / tables::FOCAL;
    let ahead = angle as f64 * 2.0 * PI / 256.0;
    let direction = ahead + across.atan();
    let (step_x, step_y) = (direction.cos(), direction.sin());
    let mut length = 0.0;
    while maze[(y + length * step_y) as usize * 16 + (x + length * step_x) as usize] == 0 {
        length += 1.0 / 512.0;
    }
    // Height depends on the distance straight ahead, not along the ray.
    let straight = length * (direction - ahead).cos();
    (tables::WALL / straight).clamp(1.0, tables::MAX_HALF_HEIGHT)
}

/// The range of heights `geometry` gives when the viewpoint is moved by up
/// to 1/128 of a cell and the ray by up to half a screen column.
///
/// That is the precision the program works with: positions have 8 bits
/// after the point, directions come in 1024ths of a turn. Away from corners
/// the range is a fraction of a pixel wide; where a ray grazes a corner it
/// spans both walls, since which of them is hit depends on less than that.
fn plausible(maze: &[u8], view: View, c: usize) -> (f64, f64) {
    let (x, y) = (view.x as f64 / 256.0, view.y as f64 / 256.0);
    let nudge = 1.0 / 128.0;
    let (mut least, mut most) = (f64::MAX, f64::MIN);
    for (dx, dy) in [
        (0.0, 0.0),
        (-nudge, -nudge),
        (-nudge, nudge),
        (nudge, -nudge),
        (nudge, nudge),
    ] {
        for dc in [-0.5, 0.0, 0.5] {
            let h = geometry(maze, x + dx, y + dy, view.angle, c as f64 + dc);
            least = least.min(h);
            most = most.max(h);
        }
    }
    (least, most)
}

// ───────────────────────────── tests ─────────────────────────────

/// Places and directions that cover all four quadrants, walls near and far,
/// and both ends of the tables.
fn views() -> Vec<View> {
    let mut views = Vec::new();
    // The middle of some open cells, looking all around.
    for (cx, cy) in [(1u16, 1u16), (5, 5), (14, 13), (8, 9), (3, 11)] {
        for angle in (0..=255u8).step_by(15) {
            views.push(View {
                x: cx << 8 | 0x80,
                y: cy << 8 | 0x80,
                angle,
            });
        }
    }
    // Off-centre, close to walls and exactly along the axes.
    for (x, y) in [
        (0x0120, 0x01E0),
        (0x01F0, 0x0110),
        (0x0E10, 0x0DF0),
        (0x0980, 0x0130),
    ] {
        for angle in [
            0, 1, 31, 32, 33, 63, 64, 65, 96, 127, 128, 129, 160, 191, 192, 193, 224, 255,
        ] {
            views.push(View { x, y, angle });
        }
    }
    views
}

/// Put the player there and let the program draw the next frame.
fn draw(machine: &mut Machine, program: &Assembly, view: View) -> u64 {
    poke16(machine, symbol(program, "_player_x"), view.x);
    poke16(machine, symbol(program, "_player_y"), view.y);
    machine
        .board_mut()
        .write(symbol(program, "_player_angle"), view.angle);
    run_frames(machine, 1, 2_000_000)
}

#[test]
fn the_program_draws_exactly_what_the_model_computes() {
    let program = build("maze3d");
    let maze = maze(&program);
    let mut machine = boot(&program, EngineKind::Fast);
    run_frames(&mut machine, 1, 2_000_000);
    for view in views() {
        draw(&mut machine, &program, view);
        let expected = paint(&model(&maze, view));
        assert!(
            machine.board().framebuffer() == expected,
            "{view:?}\nthe program drew\n{}the model says\n{}",
            picture(machine.board().framebuffer()),
            picture(&expected)
        );
    }
}

#[test]
fn the_gate_level_cpu_draws_the_same_frames() {
    let program = build("maze3d");
    let maze = maze(&program);
    let mut machine = boot(&program, EngineKind::Gate);
    run_frames(&mut machine, 1, 2_000_000);
    for view in views().into_iter().step_by(23) {
        draw(&mut machine, &program, view);
        assert!(
            machine.board().framebuffer() == paint(&model(&maze, view)),
            "{view:?}"
        );
    }
}

/// The model against floating point: every column of every view is within
/// a pixel of what plain geometry gives for that viewpoint, to the precision
/// the viewpoint is known with.
#[test]
fn the_model_agrees_with_plain_geometry() {
    let program = build("maze3d");
    let maze = maze(&program);
    let (mut columns, mut exact, mut wide) = (0, 0, 0);
    for view in views() {
        for (c, &(half, _)) in model(&maze, view).iter().enumerate() {
            let (least, most) = plausible(&maze, view, c);
            let half = half as f64;
            assert!(
                half >= least.floor() - 1.0 && half <= most.ceil() + 1.0,
                "{view:?}, column {c}: the model says {half}, geometry {least:.2}…{most:.2}"
            );
            columns += 1;
            exact += (half >= least.round() && half <= most.round()) as u32;
            wide += (most - least > 2.0) as u32;
        }
    }
    // The allowance above is rarely needed, and the range rarely wide: this
    // is a comparison, not a formality.
    println!("{columns} columns: {exact} need no allowance, {wide} graze a corner");
    assert!(
        exact * 100 >= columns * 97,
        "only {exact} of {columns} need no allowance"
    );
    assert!(
        wide * 100 <= columns * 3,
        "{wide} of {columns} ranges are wide"
    );
}

/// The first frame, as a picture a person can check: the view east from the
/// north-west corner, down a corridor seven cells long.
#[test]
fn the_first_frame_is_the_corridor() {
    let program = build("maze3d");
    let mut machine = boot(&program, EngineKind::Fast);
    run_frames(&mut machine, 1, 2_000_000);
    let drawn = picture(machine.board().framebuffer());
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../programs/maze3d/first-frame.txt"
    );
    let expected = std::fs::read_to_string(path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    if drawn != expected {
        if std::env::var_os("SILICON_BLESS").is_some() {
            std::fs::write(path, &drawn).expect("the picture can be written");
        } else {
            panic!("the first frame changed (SILICON_BLESS=1 accepts it):\n{drawn}");
        }
    }
}

/// What a frame costs, and where. This is the measurement M3 began with:
/// the nominal clock rate and the question "is the instruction set good
/// enough" both hang on it (ADR 0010, which quotes these numbers; run with
/// `--nocapture` to see them).
#[test]
fn a_frame_costs_what_adr_0010_says() {
    let program = build("maze3d");
    let mut machine = boot(&program, EngineKind::Fast);
    run_frames(&mut machine, 1, 2_000_000);
    let costs: Vec<u64> = views()
        .into_iter()
        .map(|view| draw(&mut machine, &program, view))
        .collect();
    let (least, most) = (*costs.iter().min().unwrap(), *costs.iter().max().unwrap());
    let mean = costs.iter().sum::<u64>() / costs.len() as u64;
    println!("cycles per frame: least {least}, mean {mean}, most {most}");
    assert!(
        (75_000..85_000).contains(&least),
        "the cheapest frame: {least} cycles"
    );
    assert!(
        (85_000..100_000).contains(&most),
        "the dearest frame: {most} cycles"
    );

    // The same frames again, by part of the renderer.
    let parts = [
        ("_render", "setting up the frame"),
        ("_render.column", "setting up a ray"),
        ("_render.step", "walking the map"),
        ("_render.distance", "distance to height"),
        ("_render.next", "writing the screen"),
        ("_render.new_quadrant", "changing quadrant"),
        ("mulhi", "multiplying"),
        ("wall_bits", ""),
    ];
    let names: Vec<&str> = parts.iter().map(|(name, _)| *name).collect();
    let mut sums = vec![0u64; parts.len()];
    let mut total = 0;
    for view in views() {
        poke16(&mut machine, symbol(&program, "_player_x"), view.x);
        poke16(&mut machine, symbol(&program, "_player_y"), view.y);
        machine
            .board_mut()
            .write(symbol(&program, "_player_angle"), view.angle);
        let (cycles, all) = profile(&mut machine, &program, &names, 1);
        for (sum, c) in sums.iter_mut().zip(cycles) {
            *sum += c;
        }
        total += all;
    }
    let frames = views().len() as u64;
    for ((_, what), sum) in parts.iter().zip(&sums) {
        if !what.is_empty() {
            println!(
                "{:>8} cycles  {:>3}%  {what}",
                sum / frames,
                sum * 100 / total
            );
        }
    }
    // The inner loops are where the time is: if this stops being true, the
    // numbers in the ADR are stale.
    let share = |part: usize| sums[part] * 100 / total;
    assert!(
        (30..45).contains(&share(4)),
        "writing the screen: {}%",
        share(4)
    );
    assert!((15..30).contains(&share(6)), "multiplying: {}%", share(6));
}
