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
