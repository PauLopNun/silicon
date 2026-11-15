//! Every program builds and does what it says, on both engines.

mod common;

use common::{boot, build, run_frames};
use silicon_asm::Files;
use silicon_machine::board::{key, map};
use silicon_machine::{demo, EngineKind};
use silicon_programs::{Sources, FILES, PROGRAMS};
use std::path::Path;

const ENGINES: [EngineKind; 2] = [EngineKind::Fast, EngineKind::Gate];

#[test]
fn every_program_builds_and_fits_in_rom() {
    for program in PROGRAMS {
        let assembly = build(program.name);
        assert!(!assembly.rom.is_empty(), "{}", program.name);
        assert!(assembly.rom.len() <= map::ROM_SIZE, "{}", program.name);
    }
}

/// `FILES` is written by hand; this is what notices a file left out of it,
/// or a stale entry.
#[test]
fn the_embedded_sources_are_exactly_the_files_of_the_programs_directory() {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .expect("programs/ is readable")
            .map(|e| e.expect("a directory entry"))
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let name = entry.file_name().into_string().expect("a UTF-8 file name");
            if entry.file_type().expect("a file type").is_dir() {
                walk(&entry.path(), &format!("{prefix}{name}/"), out);
            } else if !name.ends_with(".txt") {
                // (.txt: the pictures the tests compare screens with.)
                out.push(format!("{prefix}{name}"));
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../programs");
    let mut on_disk = Vec::new();
    walk(&root, "", &mut on_disk);
    on_disk.sort();
    let embedded: Vec<&str> = FILES.iter().map(|(name, _)| *name).collect();
    assert_eq!(embedded, on_disk);
    assert!(Sources.read("lib/silicon.inc").is_ok());
    assert!(Sources.read("lib/nothing.inc").is_err());
}

/// The include file is the memory map of the machine, written a second
/// time. Keep the two from drifting apart.
#[test]
fn the_include_file_agrees_with_the_machine() {
    let source = "
        .include \"lib/silicon.inc\"
        .word SCREEN, KEYS, FRAME, TIMER_LO, TIMER_HI, OUT
        .byte SCREEN_WIDTH, SCREEN_HEIGHT, ROW_BYTES
        .byte KEY_UP, KEY_DOWN, KEY_LEFT, KEY_RIGHT, KEY_A, KEY_B, KEY_START, KEY_SELECT
    ";
    struct WithTest<'a>(&'a str);
    impl Files for WithTest<'_> {
        fn read(&self, path: &str) -> Result<String, String> {
            if path == "test.asm" {
                Ok(self.0.to_string())
            } else {
                Sources.read(path)
            }
        }
    }
    let rom = silicon_asm::assemble("test.asm", &WithTest(source))
        .unwrap()
        .rom;
    let words: Vec<u16> = rom[..12]
        .chunks(2)
        .map(|w| u16::from_le_bytes([w[0], w[1]]))
        .collect();
    assert_eq!(
        words,
        [
            map::FB_START,
            map::KEYS,
            map::FRAME,
            map::TIMER_LO,
            map::TIMER_HI,
            map::OUT
        ]
    );
    assert_eq!(
        rom[12..15],
        [
            map::SCREEN_WIDTH as u8,
            map::SCREEN_HEIGHT as u8,
            map::ROW_BYTES as u8
        ]
    );
    assert_eq!(
        rom[15..],
        [
            key::UP,
            key::DOWN,
            key::LEFT,
            key::RIGHT,
            key::A,
            key::B,
            key::START,
            key::SELECT
        ]
    );
    assert_eq!(silicon_asm::ROM_SIZE, map::ROM_SIZE);
    assert_eq!(silicon_asm::RAM_START, map::RAM_START);
}

#[test]
fn hello_prints_its_line_and_stops() {
    let program = build("hello");
    for engine in ENGINES {
        let mut machine = boot(&program, engine);
        machine.run_until_halt(10_000);
        assert!(machine.halted(), "{engine:?}");
        assert_eq!(machine.board().console(), b"Hello from Silicon!\n");
    }
}

/// The assembler's first real program: the demo that M2 had to build
/// instruction by instruction in Rust. The same text must give the same
/// bytes.
#[test]
fn stripes_in_assembly_is_byte_for_byte_the_rust_built_demo() {
    assert_eq!(build("stripes").rom, demo::stripes());
}

#[test]
fn stripes_draws_the_reference_picture() {
    let program = build("stripes");
    for engine in ENGINES {
        for (keys, offset) in [(0, 2u8), (key::LEFT, 254)] {
            let mut machine = boot(&program, engine);
            machine.board_mut().set_keys(keys);
            run_frames(&mut machine, 3, 200_000);
            assert!(
                machine.board().framebuffer() == demo::stripes_frame(offset),
                "{engine:?}, keys {keys:#04x}"
            );
        }
    }
}

/// `lib/silicon.h` is the memory map a third time, for C. It must say what
/// the machine does, too.
#[test]
fn the_c_header_agrees_with_the_machine() {
    let source = "
        #include \"lib/silicon.h\"
        const u16 addresses[] = {
            (u16)SCREEN, (u16)KEYS, (u16)FRAME, (u16)TIMER_LO, (u16)TIMER_HI, (u16)OUT,
        };
        const u8 numbers[] = {
            SCREEN_WIDTH, SCREEN_HEIGHT, ROW_BYTES,
            KEY_UP, KEY_DOWN, KEY_LEFT, KEY_RIGHT, KEY_A, KEY_B, KEY_START, KEY_SELECT,
        };
        void main() { }
    ";
    struct WithTest<'a>(&'a str);
    impl Files for WithTest<'_> {
        fn read(&self, path: &str) -> Result<String, String> {
            if path == "test.c" {
                Ok(self.0.to_string())
            } else {
                Sources.read(path)
            }
        }
    }
    let program = silicon_cc::build("test.c", &WithTest(source)).unwrap();
    let at = program.symbols["_addresses"] as usize;
    let addresses: Vec<u16> = program.rom[at..at + 12]
        .chunks(2)
        .map(|w| u16::from_le_bytes([w[0], w[1]]))
        .collect();
    assert_eq!(
        addresses,
        [
            map::FB_START,
            map::KEYS,
            map::FRAME,
            map::TIMER_LO,
            map::TIMER_HI,
            map::OUT
        ]
    );
    let at = program.symbols["_numbers"] as usize;
    assert_eq!(
        program.rom[at..at + 11],
        [
            map::SCREEN_WIDTH as u8,
            map::SCREEN_HEIGHT as u8,
            map::ROW_BYTES as u8,
            key::UP,
            key::DOWN,
            key::LEFT,
            key::RIGHT,
            key::A,
            key::B,
            key::START,
            key::SELECT
        ]
    );
}

/// A program in C, start to finish: the primes below 1000 are the ones
/// trial division finds.
#[test]
fn primes_prints_the_primes_below_a_thousand() {
    let primes: Vec<u32> = (2..1000u32)
        .filter(|n| (2..*n).take_while(|d| d * d <= *n).all(|d| n % d != 0))
        .collect();
    let mut expected: String = primes.iter().map(|p| format!("{p} ")).collect();
    expected.push_str("\n168 primes below 1000\n");
    assert_eq!(primes.len(), 168);

    let program = build("primes");
    for engine in ENGINES {
        let mut machine = boot(&program, engine);
        machine.run_until_halt(3_000_000);
        assert!(machine.halted(), "{engine:?}");
        assert_eq!(
            String::from_utf8_lossy(machine.board().console()),
            expected,
            "{engine:?}"
        );
    }
}

/// The balls of `bounce.c`, in Rust.
struct Balls {
    x: [u8; 5],
    y: [u8; 5],
    dx: [i8; 5],
    dy: [i8; 5],
    screen: Vec<u8>,
}

impl Balls {
    const SIZE: u8 = 3;
    const SHADE: [u8; 5] = [3, 2, 3, 1, 2];

    fn new() -> Balls {
        Balls {
            x: [10, 40, 70, 100, 120],
            y: [5, 50, 20, 40, 10],
            dx: [1, -1, 2, -2, 1],
            dy: [1, 1, -1, 1, -2],
            screen: vec![0; map::FB_SIZE],
        }
    }

    fn ball(&mut self, n: usize, shade: u8) {
        for j in 0..Self::SIZE {
            for i in 0..Self::SIZE {
                let (x, y) = ((self.x[n] + i) as usize, (self.y[n] + j) as usize);
                let byte = &mut self.screen[y * map::ROW_BYTES + x / 4];
                *byte = *byte & !(3 << (2 * (x % 4))) | shade << (2 * (x % 4));
            }
        }
    }

    /// One step along one axis, turning round at 0 and at `last`.
    fn step(at: u8, speed: &mut i8, last: usize) -> u8 {
        let mut next = at as i32 + *speed as i32;
        if next < 0 || next > last as i32 {
            *speed = -*speed;
            next = at as i32 + *speed as i32;
        }
        next as u8
    }

    fn frame(&mut self, trails: bool) {
        if !trails {
            for n in 0..5 {
                self.ball(n, 0);
            }
        }
        for n in 0..5 {
            let size = Self::SIZE as usize;
            self.x[n] = Self::step(self.x[n], &mut self.dx[n], map::SCREEN_WIDTH - size);
            self.y[n] = Self::step(self.y[n], &mut self.dy[n], map::SCREEN_HEIGHT - size);
            self.ball(n, Self::SHADE[n]);
        }
    }
}

#[test]
fn bounce_draws_what_the_reference_does() {
    let program = build("bounce");
    let mut machine = boot(&program, EngineKind::Fast);
    let mut balls = Balls::new();
    let mut turns = 0;
    // Long enough for every ball to meet every edge; A is held for a while
    // in the middle, and the trails it leaves stay when it is let go.
    for frame in 0..400 {
        let trails = (150..230).contains(&frame);
        let before = (balls.dx, balls.dy);
        balls.frame(trails);
        turns += (before != (balls.dx, balls.dy)) as u32;
        machine
            .board_mut()
            .set_keys(if trails { key::A } else { 0 });
        run_frames(&mut machine, 1, 200_000);
        assert!(
            machine.board().framebuffer() == balls.screen,
            "frame {frame}\nthe program drew\n{}the reference says\n{}",
            common::picture(machine.board().framebuffer()),
            common::picture(&balls.screen)
        );
    }
    assert!(turns > 30, "the balls turned round {turns} times");
    // A frame for every tick of the timer: the program keeps up with it.
    let cycles = machine.board().cycles();
    assert!(
        (399 * 65_536..=400 * 65_536).contains(&cycles),
        "400 frames took {cycles} cycles"
    );
}

#[test]
fn bounce_is_the_same_on_the_gates() {
    let program = build("bounce");
    let mut machine = boot(&program, EngineKind::Gate);
    let mut balls = Balls::new();
    for frame in 0..4 {
        balls.frame(false);
        run_frames(&mut machine, 1, 200_000);
        assert!(
            machine.board().framebuffer() == balls.screen,
            "frame {frame}"
        );
    }
}

/// The page shows the screen as it is at every moment, not only when a
/// frame is finished, so a ball must never be off the screen, not even
/// while it is being moved. (The program used to rub all the balls out and
/// then draw them again, and in the browser each one was missing a quarter
/// of the time.)
#[test]
fn bounce_never_blinks() {
    let program = build("bounce");
    let mut machine = boot(&program, EngineKind::Fast);
    // Where the balls are after each frame.
    let mut balls = Balls::new();
    let mut places = vec![(balls.x, balls.y)];
    for _ in 0..60 {
        balls.frame(false);
        places.push((balls.x, balls.y));
    }
    run_frames(&mut machine, 1, 200_000);
    let mut samples = 0;
    while machine.board().frames() < 59 {
        machine.run(53);
        // Between two frames each ball is where it was or where it will
        // be, and all of it is there.
        let done = machine.board().frames() as usize;
        let board = machine.board();
        let whole = |x: u8, y: u8| {
            (0..9).all(|i| board.pixel((x + i % 3) as usize, (y + i / 3) as usize) != 0)
        };
        for n in 0..5 {
            let (was, is) = (places[done], places[done + 1]);
            assert!(
                whole(was.0[n], was.1[n]) || whole(is.0[n], is.1[n]),
                "ball {n} is not all there at cycle {}:\n{}",
                board.cycles(),
                common::picture(board.framebuffer())
            );
        }
        samples += 1;
    }
    assert!(samples > 50_000, "looked {samples} times");
}
