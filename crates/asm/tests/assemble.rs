//! The assembler from the outside: text in, ROM image or diagnostics out.

use proptest::prelude::*;
use silicon_asm::{assemble, assemble_source, Assembly, Diagnostic};
use silicon_isa::{form, Insn};

fn asm(source: &str) -> Assembly {
    match assemble_source("test.asm", source) {
        Ok(assembly) => assembly,
        Err(errors) => panic!("{}", report(&errors)),
    }
}

fn report(errors: &[Diagnostic]) -> String {
    let lines: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
    lines.join("\n")
}

/// The errors of a source that must not assemble, as `line:col: message`.
fn errors(source: &str) -> Vec<String> {
    let errors = assemble_source("test.asm", source).expect_err("the source should be rejected");
    errors
        .iter()
        .map(|e| format!("{}:{}: {}", e.pos.line, e.pos.col, e.message))
        .collect()
}

// ───────────────────────── the encoding ─────────────────────────

/// Every opcode, written the way the disassembler prints it, assembles back
/// to the same bytes. This is the whole instruction set, both directions.
#[test]
fn every_opcode_round_trips_through_text() {
    const AT: u16 = 0x0200;
    for opcode in 0..=255u8 {
        for operands in [
            [0x00, 0x00],
            [0x34, 0x12],
            [0x7F, 0x80],
            [0x80, 0x7F],
            [0xFF, 0xFF],
        ] {
            let bytes = [opcode, operands[0], operands[1]];
            let (insn, len) = Insn::decode(&bytes).unwrap();
            let text = insn.disassemble(AT);
            let assembled = asm(&format!(" .org {AT}\n {text}\n"));
            assert_eq!(
                assembled.rom[AT as usize..],
                bytes[..len],
                "`{text}` (opcode {opcode:#04x})"
            );
        }
    }
}

#[test]
fn mnemonics_and_registers_ignore_case_but_symbols_do_not() {
    assert_eq!(asm("LD A, [X+]\n Mov XL, a").rom, [0x09, 0x50]);
    assert_eq!(errors("Start: jmp start"), ["1:12: `start` is not defined"]);
}

proptest! {
    /// Any bytes are a program. Disassemble them, assemble the text, and the
    /// same bytes must come back.
    #[test]
    fn random_programs_round_trip(bytes in prop::collection::vec(any::<u8>(), 1..200)) {
        const AT: usize = 0x1000;
        let mut source = format!(" .org {AT}\n");
        let mut at = 0;
        while let Some((insn, len)) = Insn::decode(&bytes[at..]) {
            source.push_str(&format!(" {}\n", insn.disassemble((AT + at) as u16)));
            at += len;
        }
        let assembled = asm(&source);
        // (A lone opcode whose operand is cut off gives no instruction at all.)
        prop_assert_eq!(assembled.rom.get(AT..).unwrap_or(&[]), &bytes[..at]);
        // And the cycle table is reachable for every line that is code.
        for line in &assembled.lines {
            if line.len > 0 {
                let opcode = assembled.rom[line.addr.unwrap() as usize];
                prop_assert_eq!(form::form_of(opcode).bytes as u16, line.len);
            }
        }
    }
}

// ─────────────────────── labels and values ───────────────────────

#[test]
fn labels_work_forwards_and_backwards() {
    let program = asm("
        start:  ld   b, 3
        again:  dec  b
                bz   done
                jmp  again
        done:   hlt
    ");
    assert_eq!(program.rom, [0xD1, 3, 0xC9, 0xF0, 3, 0xF8, 2, 0, 0x00]);
    assert_eq!(program.symbols["start"], 0);
    assert_eq!(program.symbols["again"], 2);
    assert_eq!(program.symbols["done"], 8);
}

#[test]
fn local_labels_belong_to_the_label_above_them() {
    let program = asm("
        first:  ld   b, 2
        .loop:  dec  b
                bnz  .loop
        second: ld   b, 2
        .loop:  dec  b
                bnz  .loop
                jmp  first.loop
    ");
    assert_eq!(program.symbols["first.loop"], 2);
    assert_eq!(program.symbols["second.loop"], 7);
    assert_eq!(program.rom[10..], [0xF8, 2, 0]);
}

#[test]
fn constants_expressions_and_the_current_address() {
    let program = asm("
        SCREEN = 0x8000
        ROWS   = 64
        last   = SCREEN + (ROWS - 1) * 32      ; uses what is above
                ld   x, last
                ld   a, lo(table_end - table)  ; and what is below
                ld   b, hi(SCREEN)
        here:   jmp  $                          ; a loop on itself
        table:  .byte 1, 2, 3
        table_end:
        size = table_end - table
    ");
    assert_eq!(
        program.rom,
        [0x19, 0xE0, 0x87, 0xD0, 3, 0xD1, 0x80, 0xF8, 7, 0, 1, 2, 3]
    );
    assert_eq!(program.symbols["last"], 0x87E0);
    assert_eq!(program.symbols["size"], 3);
}

#[test]
fn negative_bytes_and_offsets_are_two_s_complement() {
    assert_eq!(
        asm("ld a, -1\n cmp a, -128\n .byte -2").rom,
        [0xD0, 0xFF, 0xEF, 0x80, 0xFE]
    );
    assert_eq!(
        asm("lea x, [x-1]\n lea sp, [sp-128]\n lea y, [y+127]").rom,
        [0x29, 0xFF, 0xFC, 0x80, 0x2A, 0x7F]
    );
    assert_eq!(asm(".word -1, 0xBEEF").rom, [0xFF, 0xFF, 0xEF, 0xBE]);
}

// ─────────────────────────── directives ───────────────────────────

#[test]
fn data_directives() {
    let program = asm("
        .byte 1, 'A', \"hi\\n\", 0
        .word 0x1234, table
        .fill 3, 0xEE
        .fill 2
        table:
    ");
    assert_eq!(
        program.rom,
        [1, 65, b'h', b'i', b'\n', 0, 0x34, 0x12, 15, 0, 0xEE, 0xEE, 0xEE, 0, 0]
    );
}

#[test]
fn org_and_align_leave_zeroes_and_res_names_ram() {
    let program = asm("
                jmp  main
                .org 0x10
        main:   inc  a
                .align 8
        table:  .byte 7
                .org 0x9000
        score:  .res 2
        lives:  .res 1
        buffer: .res 256
        end:
    ");
    let mut expected = vec![0xF8, 0x10, 0x00];
    expected.resize(0x10, 0);
    expected.push(0xC0);
    expected.resize(0x18, 0);
    expected.push(7);
    assert_eq!(
        program.rom, expected,
        "the image stops at the last byte of ROM"
    );
    assert_eq!(program.symbols["table"], 0x18);
    assert_eq!(program.symbols["score"], 0x9000);
    assert_eq!(program.symbols["lives"], 0x9002);
    assert_eq!(program.symbols["buffer"], 0x9003);
    assert_eq!(program.symbols["end"], 0x9103);
}

/// `.ram` and `.rom` switch between two location counters, so files can
/// declare variables next to the code that uses them.
#[test]
fn rom_and_ram_each_continue_where_they_were_left() {
    let program = asm("
                ld   a, [lives]
                .ram
        lives:  .res 1
        score:  .res 2
                .rom
                st   [score], a
                .ram
                .ram                ; already there: nothing happens
        best:   .res 2
                .rom
                hlt
    ");
    assert_eq!(program.rom, [0xFA, 0x00, 0x90, 0xFB, 0x01, 0x90, 0x00]);
    assert_eq!(program.symbols["lives"], 0x9000);
    assert_eq!(program.symbols["score"], 0x9001);
    assert_eq!(program.symbols["best"], 0x9003);
    assert_eq!(
        errors(" .ram\ncounter: .byte 1").len(),
        1,
        "RAM still cannot be given contents"
    );
}

#[test]
fn layout_can_use_what_is_already_known() {
    let program = asm("
        SIZE = 4
        a_table: .fill SIZE, 1
        b_table: .fill b_table - a_table, 2    ; as long as the first
                 .org $ + 2
        tail:    .byte 3
    ");
    assert_eq!(program.rom, [1, 1, 1, 1, 2, 2, 2, 2, 0, 0, 3]);
}

// ─────────────────────── jumps of any distance ───────────────────────

#[test]
fn a_jump_is_a_branch_when_the_target_is_near() {
    assert_eq!(
        asm("top: dec b\n jnz top\n hlt").rom,
        [0xC9, 0xF1, 0xFD, 0x00]
    );
}

#[test]
fn a_jump_goes_around_a_jmp_when_the_target_is_far() {
    let program = asm("
        top:    dec  b
                .fill 200
                jnz  top
                hlt
    ");
    // bz +3 ; jmp top
    assert_eq!(program.rom[201..], [0xF0, 3, 0xF8, 0, 0, 0x00]);
}

/// The two sizes meet exactly where a branch stops reaching.
#[test]
fn jumps_switch_encoding_exactly_at_the_reach_of_a_branch() {
    let forward = |gap: usize| asm(&format!(" jc end\n .fill {gap}\nend: hlt")).rom;
    assert_eq!(forward(127)[..2], [0xF2, 127]);
    assert_eq!(forward(128)[..5], [0xF3, 3, 0xF8, 133, 0]);
    let backward = |gap: usize| asm(&format!("top: .fill {gap}\n jge top\n hlt")).rom;
    assert_eq!(backward(126)[126..], [0xF7, 0x80, 0x00]);
    assert_eq!(backward(127)[127..], [0xF6, 3, 0xF8, 0, 0, 0x00]);
}

/// Making one jump long can push another out of reach: the layout repeats
/// until nothing changes.
#[test]
fn lengthening_one_jump_can_lengthen_another() {
    let program = asm("
                jz   near           ; 127 bytes away, until…
                jz   far            ; …this one grows by 3 bytes in between
                .fill 125
        near:   .fill 200
        far:    hlt
    ");
    assert_eq!(program.rom[..5], [0xF1, 3, 0xF8, 135, 0]);
    assert_eq!(program.rom[5..10], [0xF1, 3, 0xF8, 0x4F, 0x01]);
    assert_eq!(
        (program.symbols["near"], program.symbols["far"]),
        (135, 335)
    );
}

#[test]
fn every_condition_has_a_jump_and_its_opposite() {
    let names = ["jz", "jnz", "jc", "jnc", "jn", "jp", "jlt", "jge"];
    for (i, name) in names.iter().enumerate() {
        let near = asm(&format!(" {name} end\nend: hlt")).rom;
        assert_eq!(near, [0xF0 + i as u8, 0, 0x00], "{name}");
        let far = asm(&format!(" {name} end\n .fill 300\nend: hlt")).rom;
        assert_eq!(
            far[..5],
            [0xF0 + (i as u8 ^ 1), 3, 0xF8, 0x31, 0x01],
            "{name}"
        );
    }
}

// ──────────────────────────── includes ────────────────────────────

#[test]
fn includes_are_assembled_in_place_relative_to_the_including_file() {
    let files = [
        (
            "game/main.asm",
            " .include \"../lib/io.inc\"\n .include \"data.asm\"\nmain: ld a, [KEYS]\n ld x, table",
        ),
        ("lib/io.inc", "KEYS = 0x8800"),
        ("game/data.asm", "table: .byte 9"),
    ];
    let program = assemble("game/main.asm", &files).unwrap();
    assert_eq!(program.rom, [9, 0xFA, 0x00, 0x88, 0x19, 0x00, 0x00]);
    assert_eq!(
        program.files,
        ["game/main.asm", "lib/io.inc", "game/data.asm"]
    );
    // The source map follows the order of assembly.
    let order: Vec<(usize, u32)> = program.lines.iter().map(|l| (l.file, l.line)).collect();
    assert_eq!(order, [(0, 1), (1, 1), (0, 2), (2, 1), (0, 3), (0, 4)]);
}

#[test]
fn errors_in_an_included_file_name_that_file() {
    let files = [
        (
            "main.asm",
            " inc a\n .include \"lib.asm\"\n .include \"missing.asm\"\n",
        ),
        ("lib.asm", "\n  bogus 1\n"),
    ];
    let errors = assemble("main.asm", &files).unwrap_err();
    assert_eq!(
        report(&errors),
        "lib.asm:2:3: error: unknown instruction `bogus`\n\
         main.asm:3:2: error: cannot read `missing.asm`: no such file"
    );
}

#[test]
fn a_file_that_includes_itself_is_an_error_not_a_hang() {
    let files = [("loop.asm", " .include \"loop.asm\"")];
    let errors = assemble("loop.asm", &files).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(
        errors[0].message,
        "`loop.asm` is included from inside itself"
    );
    let missing = assemble("nothing.asm", &files).unwrap_err();
    assert_eq!(
        missing[0].to_string(),
        "nothing.asm:1:1: error: cannot read `nothing.asm`: no such file"
    );
}

#[test]
fn loc_redirects_positions_to_another_source() {
    let source = " inc a\n .loc \"game.c\", 40\n inc a\n\n bogus\n";
    let errors = assemble_source("out.asm", source).unwrap_err();
    assert_eq!(
        report(&errors),
        "game.c:42:2: error: unknown instruction `bogus`"
    );
}

/// With a column, `.loc` names one place for everything that follows: the
/// statement of a compiled language that the generated code belongs to.
#[test]
fn loc_with_a_column_pins_every_error_to_one_place() {
    let source = " .loc \"game.c\", 7, 5\n inc a\n        call _missing\n ld a, 999\n \
                  .loc \"game.c\", 9, 1\n jmp _gone\n";
    let errors = assemble_source("out.s", source).unwrap_err();
    assert_eq!(
        report(&errors),
        "game.c:7:5: error: `_missing` is not defined\n\
         game.c:7:5: error: the value is 999; it must be -128…255\n\
         game.c:9:1: error: `_gone` is not defined"
    );
}

// ──────────────────────────── diagnostics ────────────────────────────

#[test]
fn all_errors_are_reported_with_their_positions() {
    assert_eq!(
        errors(" ld a, 1\n lod a, 1\n ld a, #1\n jmp nowhere\n"),
        [
            "2:2: unknown instruction `lod`",
            "3:8: unexpected character `#`",
        ],
        "syntax errors stop the assembly before symbols are looked at"
    );
    assert_eq!(
        errors(" jmp nowhere\n ld a, [there]\n"),
        [
            "1:6: `nowhere` is not defined",
            "2:9: `there` is not defined"
        ]
    );
}

#[test]
fn operands_are_checked_against_their_range() {
    assert_eq!(
        errors(" ld a, 256"),
        ["1:8: the value is 256; it must be -128…255"]
    );
    assert_eq!(
        errors(" ld a, -129"),
        ["1:8: the value is -129; it must be -128…255"]
    );
    assert_eq!(
        errors(" ld x, 0x10000"),
        ["1:8: the value is 65536; it must be -32768…65535"]
    );
    assert_eq!(
        errors(" ld a, [sp-1]"),
        ["1:11: the offset from `sp` is -1; it must be 0…255"]
    );
    assert_eq!(errors(" ld a, [sp+256]").len(), 1);
    assert_eq!(
        errors(" lea x, [x+128]"),
        ["1:12: the offset is 128; it must be -128…127"]
    );
    assert_eq!(
        errors(" .byte 1, 300"),
        ["1:11: the byte is 300; it must be -128…255"]
    );
    assert_eq!(
        errors(" .word 70000"),
        ["1:8: the word is 70000; it must be -32768…65535"]
    );
}

#[test]
fn a_branch_that_cannot_reach_says_how_far_and_what_to_do() {
    assert_eq!(
        errors(" bnz far\n .fill 200\nfar: hlt"),
        [
            "1:6: the target is 200 bytes away and a branch reaches −128…+127; \
          write `jnz` to let the assembler use a `jmp`"
        ]
    );
}

#[test]
fn symbols_cannot_be_defined_twice_or_in_circles() {
    assert_eq!(
        errors("here: inc a\nhere: inc a"),
        ["2:1: `here` is already defined at test.asm:1"]
    );
    assert_eq!(
        errors("n = 1\nn = 2"),
        ["2:1: `n` is already defined at test.asm:1"]
    );
    assert_eq!(
        errors("a1 = b1 + 1\nb1 = a1 + 1\n ld a, a1"),
        [
            "1:6: `a1` is defined in terms of itself",
            "2:6: `b1` is defined in terms of itself",
            "3:8: `a1` is defined in terms of itself",
        ]
    );
    assert_eq!(
        errors("unused = nowhere + 1"),
        ["1:10: `nowhere` is not defined"],
        "a constant is checked even if nothing uses it"
    );
    assert_eq!(
        errors("size = count * 2\n ld a, size"),
        [
            "1:8: `count` is not defined",
            "2:8: `count` is not defined (needed by `size`)"
        ]
    );
}

#[test]
fn layout_cannot_depend_on_what_comes_later() {
    assert_eq!(
        errors(" .fill end - 4\nend:"),
        ["1:8: `end` is defined further down, so its address is not known yet here"]
    );
    assert_eq!(
        errors(" .org later\nlater = $ + 2"),
        ["1:7: `later` uses `$` and is defined further down, so its value is not known yet here"]
    );
    assert_eq!(
        errors(" .org -1"),
        ["1:7: the address is -1; it must be 0…65535"]
    );
    assert_eq!(
        errors(" .align 0"),
        ["1:9: the alignment is 0; it must be at least 1"]
    );
}

#[test]
fn only_the_rom_can_be_given_contents() {
    assert_eq!(
        errors(" .org 0x9000\ncounter: .byte 0"),
        [
            "2:10: this would be stored at 0x9000, outside the ROM (0x0000–0x7fff); \
          RAM cannot be given contents by a ROM image (use `.res` to name it)"
        ]
    );
    assert_eq!(
        errors(" .org 0x7fff\n jmp 0").len(),
        1,
        "an instruction across the end of ROM"
    );
    assert_eq!(asm(" .org 0x7fff\n hlt").rom.len(), 0x8000);
    assert_eq!(
        errors(" .org 0xffff\n .res 2"),
        ["2:2: this runs past the end of memory (0xffff)"]
    );
}

#[test]
fn two_things_cannot_be_stored_at_the_same_address() {
    assert_eq!(
        errors(" jmp 0\n .org 2\n .byte 1, 2"),
        ["3:2: this overwrites what is already at 0x0002"]
    );
}

#[test]
fn a_flood_of_errors_is_cut_short() {
    let source = " bogus\n".repeat(500);
    let errors = assemble_source("test.asm", &source).unwrap_err();
    assert_eq!(errors.len(), 51);
    assert_eq!(errors[50].message, "too many errors; stopping here");
}

// ───────────────────────────── listing ─────────────────────────────

#[test]
fn the_listing_shows_addresses_bytes_and_source() {
    let program = asm("\
; count down
COUNT = 3
start:  ld   b, COUNT
.loop:  dec  b
        jnz  .loop

text:   .byte \"hello!\", 0
        .org 0x9000
var:    .res 1
");
    let expected = r#"
                   ; count down
                   COUNT = 3
0000  d1 03        start:  ld   b, COUNT
0002  c9           .loop:  dec  b
0003  f1 fd                jnz  .loop

0005  68 65 6c 6c  text:   .byte "hello!", 0
0009  6f 21 00
9000                       .org 0x9000
9000               var:    .res 1
"#;
    assert_eq!(program.listing(), expected[1..]);
    // The source map: which line each address came from.
    let line = program.lines.iter().find(|l| l.addr == Some(2)).unwrap();
    assert_eq!((line.line, line.len), (4, 1));
}
