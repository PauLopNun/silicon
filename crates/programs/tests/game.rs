//! maze3d, the game (`programs/maze3d/main.c`): the maze it digs, how the
//! player moves, what the screen shows, and whether the way out can be
//! found with nothing but the keys.
//!
//! As with the renderer, the program is compared with the same rules
//! written in plain Rust: `dig` for the maze and `Player::frame` for the
//! walking. The picture is the renderer's model (`common::renderer`) with
//! the map drawn over its corner.

mod common;

use common::renderer::{model, paint, View};
use common::{boot, build, picture, poke16, run_frames, symbol};
use silicon_asm::Assembly;
use silicon_machine::board::{key, map};
use silicon_machine::{EngineKind, Machine};
use silicon_programs::tables;
use std::collections::VecDeque;

const START: u8 = 16 + 1;
const EXIT: u8 = 13 * 16 + 13;
/// One cell east, south, west, north.
const BESIDE: [i8; 4] = [1, 16, -1, -16];
const TURN: u8 = 3;
const MARGIN: u16 = 48;
/// More than any frame takes, digging a maze included.
const FRAME_LIMIT: u64 = 3_000_000;

// ───────────────────────── the rules, in Rust ─────────────────────────

/// The game's random numbers: a 16-bit xorshift.
struct Random(u16);

impl Random {
    fn next(&mut self) -> u8 {
        self.0 ^= self.0 << 7;
        self.0 ^= self.0 >> 9;
        self.0 ^= self.0 << 8;
        self.0 as u8
    }
}

/// The maze of a level.
fn dig(level: u8) -> Vec<u8> {
    let mut random = Random(0x5eed + level as u16);
    for _ in 0..8 {
        random.next();
    }
    let mut maze = vec![1u8; 256];
    let (mut cell, mut path) = (START, Vec::new());
    maze[cell as usize] = 0;
    loop {
        let mut d = random.next();
        let mut next = None;
        for _ in 0..4 {
            d = d.wrapping_add(1) & 3;
            let room = cell.wrapping_add((BESIDE[d as usize] * 2) as u8);
            if room & 15 > 13 || room >= 14 * 16 {
                continue;
            }
            if maze[room as usize] != 0 {
                next = Some(room);
                break;
            }
        }
        match next {
            Some(room) => {
                maze[cell.wrapping_add(BESIDE[d as usize] as u8) as usize] = 0;
                maze[room as usize] = 0;
                path.push(cell);
                cell = room;
            }
            None => match path.pop() {
                Some(back) => cell = back,
                None => return maze,
            },
        }
    }
}

/// The cells from `from` to `to`, one step at a time, by the shortest way.
fn way(maze: &[u8], from: u8, to: u8) -> Option<Vec<u8>> {
    let mut came_from = [None; 256];
    let mut queue = VecDeque::from([from]);
    came_from[from as usize] = Some(from);
    while let Some(cell) = queue.pop_front() {
        if cell == to {
            let mut way = vec![to];
            while *way.last().unwrap() != from {
                way.push(came_from[*way.last().unwrap() as usize].unwrap());
            }
            way.reverse();
            return Some(way);
        }
        for step in BESIDE {
            let next = cell.wrapping_add(step as u8);
            if maze[next as usize] == 0 && came_from[next as usize].is_none() {
                came_from[next as usize] = Some(cell);
                queue.push_back(next);
            }
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Player {
    x: u16,
    y: u16,
    angle: u8,
}

impl Player {
    /// Where a level begins: the middle of the first room, looking along
    /// the passage that leaves it.
    fn at_start(maze: &[u8]) -> Player {
        Player {
            x: 0x0180,
            y: 0x0180,
            angle: if maze[START as usize + 1] != 0 { 64 } else { 0 },
        }
    }

    fn cell(&self) -> u8 {
        cell_at(self.x, self.y)
    }

    fn view(&self) -> View {
        View {
            x: self.x,
            y: self.y,
            angle: self.angle,
        }
    }

    /// What one frame with these keys held does.
    fn frame(&mut self, keys: u8, maze: &[u8]) {
        if keys & key::LEFT != 0 {
            self.angle = self.angle.wrapping_sub(TURN);
        }
        if keys & key::RIGHT != 0 {
            self.angle = self.angle.wrapping_add(TURN);
        }
        if keys & (key::UP | key::DOWN) == 0 {
            return;
        }
        let walk = tables::walk();
        let mut dx = walk[self.angle as usize];
        let mut dy = walk[self.angle.wrapping_sub(64) as usize];
        if keys & key::DOWN != 0 {
            (dx, dy) = (-dx, -dy);
        }
        let x = self.x.wrapping_add(dx as u16);
        if !blocked(maze, x, self.y) {
            self.x = x;
        }
        let y = self.y.wrapping_add(dy as u16);
        if !blocked(maze, self.x, y) {
            self.y = y;
        }
    }
}

fn cell_at(x: u16, y: u16) -> u8 {
    ((y >> 8) as u8) << 4 | (x >> 8) as u8
}

/// Is a corner of the player's square in a wall?
fn blocked(maze: &[u8], x: u16, y: u16) -> bool {
    [(x - MARGIN, y - MARGIN), (x + MARGIN, y - MARGIN)]
        .into_iter()
        .chain([(x - MARGIN, y + MARGIN), (x + MARGIN, y + MARGIN)])
        .any(|(x, y)| maze[cell_at(x, y) as usize] != 0)
}

/// The screen: the view, with the map over its top left corner.
fn screen(maze: &[u8], player: Player) -> Vec<u8> {
    let mut screen = paint(&model(maze, player.view()));
    for cell in 0..=255u8 {
        let shade = match cell {
            _ if cell == player.cell() => 3,
            EXIT => 2,
            _ if maze[cell as usize] != 0 => 1,
            _ => 0,
        };
        let (x, y) = ((cell & 15) as usize, (cell >> 4) as usize);
        let byte = &mut screen[y * map::ROW_BYTES + x / 4];
        *byte = *byte & !(3 << (2 * (x % 4))) | shade << (2 * (x % 4));
    }
    screen
}

// ─────────────────────────── the program ───────────────────────────

struct Game {
    program: Assembly,
    machine: Machine,
}

impl Game {
    fn new(engine: EngineKind) -> Game {
        let program = build("maze3d");
        let machine = boot(&program, engine);
        Game { program, machine }
    }

    fn byte(&self, name: &str) -> u8 {
        self.machine.board().peek(symbol(&self.program, name))
    }

    fn word(&self, name: &str) -> u16 {
        let at = symbol(&self.program, name);
        let board = self.machine.board();
        u16::from_le_bytes([board.peek(at), board.peek(at + 1)])
    }

    fn maze(&self) -> Vec<u8> {
        let at = symbol(&self.program, "_maze_map");
        (0..256)
            .map(|i| self.machine.board().peek(at + i))
            .collect()
    }

    fn player(&self) -> Player {
        Player {
            x: self.word("_player_x"),
            y: self.word("_player_y"),
            angle: self.byte("_player_angle"),
        }
    }

    fn console(&self) -> String {
        String::from_utf8_lossy(self.machine.board().console()).into_owned()
    }

    /// Hold these keys and let the program draw one frame; returns the
    /// clock cycles it took.
    fn frame(&mut self, keys: u8) -> u64 {
        self.machine.board_mut().set_keys(keys);
        run_frames(&mut self.machine, 1, FRAME_LIMIT)
    }
}

// ───────────────────────────── the maze ─────────────────────────────

/// Every maze the game can dig (the level number is a byte) is a maze: walls
/// all around, as the renderer needs, every room reached, and exactly one
/// way between any two rooms.
#[test]
fn every_level_is_a_maze_with_a_way_out() {
    let mut longest = 0;
    for level in 0..=255u8 {
        let maze = dig(level);
        for i in 0..16 {
            for edge in [
                i,
                15 * 16 + i,
                i * 16,
                i * 16 + 15,
                14 * 16 + i,
                i * 16 + 14,
            ] {
                assert_eq!(maze[edge], 1, "level {level}: the edge is open at {edge}");
            }
        }
        // 49 rooms joined into a tree by 48 passages, and nothing else.
        let open = maze.iter().filter(|&&c| c == 0).count();
        assert_eq!(open, 49 + 48, "level {level}");
        for row in (1..14).step_by(2) {
            for column in (1..14).step_by(2) {
                let room = (row * 16 + column) as u8;
                assert!(
                    way(&maze, START, room).is_some(),
                    "level {level}: room {room}"
                );
            }
        }
        // Straight to the opposite corner is 25 cells; no way out is
        // shorter.
        let out = way(&maze, START, EXIT).expect("a way out").len();
        assert!(out >= 25 && out % 2 == 1, "level {level}: {out} cells");
        longest = longest.max(out);
    }
    println!("the longest way out is {longest} cells");
}

#[test]
fn levels_differ() {
    let mazes: Vec<Vec<u8>> = (1..=40).map(dig).collect();
    for (i, a) in mazes.iter().enumerate() {
        for b in &mazes[..i] {
            assert_ne!(a, b);
        }
    }
}

#[test]
fn the_game_digs_the_maze_of_the_reference() {
    let mut game = Game::new(EngineKind::Fast);
    game.frame(0);
    assert_eq!(game.console(), "Maze 1\n");
    assert_eq!(game.maze(), dig(1));
    assert_eq!(game.player(), Player::at_start(&dig(1)));
    // The next levels, reached by being put on the way out.
    for level in 2..=6u8 {
        poke16(
            &mut game.machine,
            symbol(&game.program, "_player_x"),
            0x0d80,
        );
        poke16(
            &mut game.machine,
            symbol(&game.program, "_player_y"),
            0x0d80,
        );
        // The shutter's eight frames, and the first of the new level.
        for _ in 0..8 {
            game.frame(0);
            assert_eq!(game.byte("_level"), level - 1);
        }
        game.frame(0);
        assert_eq!(game.byte("_level"), level);
        assert_eq!(game.maze(), dig(level), "level {level}");
        assert_eq!(game.player(), Player::at_start(&dig(level)));
    }
    assert_eq!(
        game.console(),
        "Maze 1\nMaze 2\nMaze 3\nMaze 4\nMaze 5\nMaze 6\n"
    );
}

// ──────────────────────── walking and the screen ────────────────────────

/// Keys for a walk around the first maze that turns, walks into walls
/// head-on and at an angle, backs off, and stands still.
fn script() -> Vec<u8> {
    let mut keys = Vec::new();
    for (held, frames) in [
        (0, 2),
        (key::UP, 30),
        (key::UP | key::RIGHT, 25),
        (key::RIGHT, 12),
        (key::UP, 40),
        (key::DOWN, 15),
        (key::LEFT, 40),
        (key::UP | key::LEFT, 30),
        (key::UP | key::DOWN, 5),
        (key::A | key::B | key::START | key::SELECT, 3),
        (key::UP, 60),
        (key::DOWN | key::RIGHT, 45),
        (key::LEFT | key::RIGHT, 4),
        (key::UP, 50),
    ] {
        keys.extend(std::iter::repeat_n(held, frames));
    }
    keys
}

#[test]
fn the_player_walks_as_the_reference_says_and_the_screen_shows_it() {
    let mut game = Game::new(EngineKind::Fast);
    let maze = dig(1);
    let mut player = Player::at_start(&maze);
    let (mut moved, mut stopped) = (0, 0);
    for (i, keys) in script().into_iter().enumerate() {
        let before = player;
        player.frame(keys, &maze);
        game.frame(keys);
        assert_eq!(game.player(), player, "frame {i}, keys {keys:#04x}");
        let expected = screen(&maze, player);
        assert!(
            game.machine.board().framebuffer() == expected,
            "frame {i}, {player:?}\nthe program drew\n{}the reference says\n{}",
            picture(game.machine.board().framebuffer()),
            picture(&expected)
        );
        let walking = keys & (key::UP | key::DOWN) != 0;
        moved += (walking && (player.x, player.y) != (before.x, before.y)) as u32;
        stopped += (walking && (player.x == before.x || player.y == before.y)) as u32;
    }
    // The walk did both: moved freely and was held by walls.
    assert!(moved > 150, "moved in {moved} frames");
    assert!(stopped > 60, "held by a wall in {stopped} frames");
    assert_ne!(player.cell(), START);
}

/// One computer stays on the fast engine; its twin is handed to the gates
/// for the first frame (which digs the maze) and for one frame in every
/// twenty-nine of the walk, in the middle of the game, and back.
#[test]
fn the_gate_level_cpu_plays_the_same_game() {
    let mut fast = Game::new(EngineKind::Fast);
    let mut twin = Game::new(EngineKind::Fast);
    let mut on_gates = 0;
    for (i, held) in script().into_iter().enumerate() {
        let gates = i % 29 == 0;
        twin.machine.set_engine(if gates {
            EngineKind::Gate
        } else {
            EngineKind::Fast
        });
        assert_eq!(twin.frame(held), fast.frame(held), "frame {i}");
        if gates {
            on_gates += 1;
            assert_eq!(twin.machine.state(), fast.machine.state(), "frame {i}");
            assert!(
                twin.machine.board().framebuffer() == fast.machine.board().framebuffer(),
                "frame {i}"
            );
        }
    }
    assert!(on_gates >= 12);
    assert!(twin.machine.board().ram() == fast.machine.board().ram());
    assert_eq!(twin.player(), fast.player());
}

/// The first frame, as a picture a person can check: the passage that
/// leaves the first room, and the map in the corner.
#[test]
fn the_first_frame_is_the_first_passage_with_the_map_over_it() {
    let mut game = Game::new(EngineKind::Fast);
    game.frame(0);
    let drawn = picture(game.machine.board().framebuffer());
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../programs/maze3d/level-1.txt"
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

// ─────────────────────────── finding the way out ───────────────────────────

/// The keys a careful player would hold to get from where they are to the
/// middle of the cell `target`, which is beside the one they are in: turn
/// until facing it, then walk.
fn steer(player: Player, target: u8) -> u8 {
    let (tx, ty) = (
        (target & 15) as i32 * 256 + 128,
        (target >> 4) as i32 * 256 + 128,
    );
    let (dx, dy) = (tx - player.x as i32, ty - player.y as i32);
    let heading: u8 = match (dx.abs() > dy.abs(), dx > 0, dy > 0) {
        (true, true, _) => 0,
        (true, false, _) => 128,
        (false, _, true) => 64,
        (false, _, false) => 192,
    };
    match heading.wrapping_sub(player.angle) as i8 {
        -1..=1 => key::UP,
        2.. => key::RIGHT,
        _ => key::LEFT,
    }
}

/// The game can be won with the keys alone: a player who follows the map
/// gets out of the first three mazes. On the way, every frame costs about
/// what the renderer does, so the game runs at the speed ADR 0010 measured.
#[test]
fn following_the_map_leads_out_of_the_maze() {
    let mut game = Game::new(EngineKind::Fast);
    game.frame(0);
    let (mut frames, mut cycles, mut dearest) = (0u64, 0u64, 0u64);
    for level in 1..=3u8 {
        let maze = game.maze();
        assert_eq!(maze, dig(level));
        let way = way(&maze, START, EXIT).expect("there is a way out");
        let mut next = 1;
        while game.byte("_level") == level {
            let player = game.player();
            // Near enough to the middle of the cell aimed for: aim for the
            // one after it.
            let (tx, ty) = (
                (way[next] & 15) as i32 * 256 + 128,
                (way[next] >> 4) as i32 * 256 + 128,
            );
            let off = (tx - player.x as i32)
                .abs()
                .max((ty - player.y as i32).abs());
            if off <= 12 && next + 1 < way.len() {
                next += 1;
            }
            // Once in the last room there is nothing left to do but watch
            // the shutter close.
            let arrived = player.cell() == EXIT;
            let keys = if arrived { 0 } else { steer(player, way[next]) };
            let cost = game.frame(keys);
            if !arrived {
                (frames, cycles, dearest) = (frames + 1, cycles + cost, dearest.max(cost));
            }
            assert!(frames < 20_000, "level {level}: lost at {player:?}");
        }
        // The shutter has closed and the next maze is on the screen.
        assert_eq!(game.player(), Player::at_start(&dig(level + 1)));
    }
    assert_eq!(game.console(), "Maze 1\nMaze 2\nMaze 3\nMaze 4\n");
    let mean = cycles / frames;
    println!(
        "{frames} frames walked: mean {mean} cycles ({:.0} per second at 4 MHz), dearest {dearest}",
        4e6 / mean as f64
    );
    assert!(
        (85_000..100_000).contains(&mean),
        "a frame costs {mean} cycles"
    );
    assert!(
        dearest < 110_000,
        "the dearest frame costs {dearest} cycles"
    );
}
