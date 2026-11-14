//! Programs that must not compile, and what the programmer is told: where
//! the mistake is, and what it is, in a sentence.

mod common;

use common::errors;

/// The one error of a `main` made of these statements, as `column: message`
/// (the statements are line 3 of the program).
fn in_main(body: &str) -> String {
    let all = errors(&format!("#include \"test.h\"\nvoid main() {{\n{body}\n}}"));
    assert_eq!(all.len(), 1, "{body}: {all:?}");
    all[0]
        .strip_prefix("main.c:3:")
        .unwrap_or_else(|| panic!("{body}: not on line 3: {}", all[0]))
        .to_string()
}

/// The one error of a whole program, as `line:column: message`.
fn in_program(source: &str) -> String {
    let all = errors(source);
    assert_eq!(all.len(), 1, "{source}: {all:?}");
    all[0]
        .strip_prefix("main.c:")
        .unwrap_or_else(|| panic!("{source}: not in main.c: {}", all[0]))
        .to_string()
}

#[test]
fn names() {
    assert_eq!(in_main("x = 1;"), "1: `x` is not declared");
    assert_eq!(in_main("nothing(1);"), "1: `nothing` is not declared");
    assert_eq!(
        in_main("u8 x; u8 x;"),
        "10: `x` is already declared in this block"
    );
    assert_eq!(in_main("u8 a; a();"), "7: `a` is not a function");
    assert_eq!(
        in_main("u8 a = print;"),
        "8: `print` is a function; call it: `print(…)`"
    );
    assert_eq!(
        in_program("u8 x; u8 x() { return 1; } void main() { }"),
        "1:10: `x` is already declared as a variable"
    );
    assert_eq!(
        in_program("void f(); u8 f; void main() { }"),
        "1:14: `f` is a function"
    );
    assert_eq!(
        in_program("u8 f(u8 a, u8 a) { return a; } void main() { }"),
        "1:15: two parameters are called `a`"
    );
    assert_eq!(
        in_program("void main() { }\nvoid main() { }"),
        "2:6: `main` is already defined at main.c:1"
    );
    assert_eq!(
        in_program("u8 f(u8 a);\nu16 f(u8 a) { return 1; }\nvoid main() { }"),
        "2:5: `f` is declared differently at main.c:1"
    );
}

#[test]
fn numbers_that_do_not_fit() {
    assert_eq!(in_main("u8 a = 300;"), "8: 300 does not fit in a `u8`");
    assert_eq!(
        in_main("i8 a = 200; i8 b = -129;"),
        "20: -129 does not fit in an `i8`"
    );
    assert_eq!(
        in_main("u16 w = 70000;"),
        "9: 70000 does not fit in a `u16`"
    );
    assert_eq!(
        in_main("u8 a = 99999 + 1;"),
        "14: 100000 does not fit in a `u8`"
    );
    assert_eq!(
        in_main("u16 a = 1 << 40;"),
        "11: this constant is too large"
    );
    assert_eq!(
        in_main("u8 a = 1; a << 300;"),
        "16: nothing can be shifted by 300"
    );
    assert_eq!(in_main("u8 a = 1 / 0;"), "10: this divides by zero");
    assert_eq!(in_main("u8 a = 5; a = a % 0;"), "17: this divides by zero");
}

#[test]
fn types_that_do_not_go_together() {
    assert_eq!(
        in_main("u8 a = 1; u16* p = &a;"),
        "20: this is a `u8*` and a `u16*` is needed; write `(u16*)…` if that is meant"
    );
    assert_eq!(
        in_main("u8* p = 5;"),
        "9: this is a number and a `u8*` is needed; write `(u8*)…` if that is meant"
    );
    assert_eq!(
        in_main("u8 a; a = \"text\";"),
        "9: this is a `u8*` and a `u8` is needed; write `(u8)…` if that is meant"
    );
    assert_eq!(
        in_main("u8 a = 1; u8* p = &a; u8* q = p + p;"),
        "33: a pointer can be moved by a number (`p + n`, `p - n`) and nothing else"
    );
    assert_eq!(
        in_main("u8 a = 1; u8* p = &a; u16 b = (u16)p * 2; b = p * 2;"),
        "49: a pointer can be moved by a number (`p + n`, `p - n`) and nothing else"
    );
    assert_eq!(
        in_main("u8 a = *5;"),
        "8: `*` follows a pointer, and this is a number"
    );
    assert_eq!(
        in_main("u8 a = 1; u8 b = a[0];"),
        "19: only an array or a pointer can be indexed"
    );
    assert_eq!(
        in_main("u8 a = putc(1);"),
        "8: this returns nothing, and a value is needed here"
    );
    assert_eq!(in_main("void v;"), "6: a variable cannot be `void`");
    assert_eq!(in_main("u8 a = (void)1;"), "8: nothing can be made `void`");
    assert_eq!(
        in_program("void* p; void main() { }"),
        "1:7: there are no pointers to `void`; use `u8*`"
    );
}

#[test]
fn places() {
    let not_a_place = "this is not a place in memory: it cannot be assigned or pointed at";
    assert_eq!(in_main("5 = 1;"), format!("1: {not_a_place}"));
    assert_eq!(in_main("u8 a; &(a + 1);"), format!("11: {not_a_place}"));
    assert_eq!(
        in_main("u8 t[4]; t = 0;"),
        "12: an array cannot be assigned as a whole; assign its elements"
    );
    assert_eq!(
        in_main("u8* p; *p++ += 1;"),
        "13: this place is found by an expression that changes something, so it cannot be \
         read and written in one go; work the place out first"
    );
    assert_eq!(
        in_main("const u8 K = 3; K = 4;"),
        "17: `K` is a constant; it has no place in memory"
    );
    assert_eq!(
        in_program("const u8 K = 3; void main() { u8* p = &K; }"),
        "1:40: `K` is a constant; it has no place in memory"
    );
}

#[test]
fn declarations() {
    assert_eq!(
        in_main("u8 t[4] = { 1, 2 };"),
        "4: an array inside a function starts with no values; assign its elements, or make it \
         a global"
    );
    assert_eq!(in_main("u8 t[];"), "4: the array `t` needs a size");
    assert_eq!(in_main("u8 t[0];"), "6: an array cannot have 0 elements");
    assert_eq!(
        in_main("u8 n = 3; u8 t[n];"),
        "16: the size of an array must be a constant"
    );
    assert_eq!(
        in_main("u8 n = 3; const u8 K = n;"),
        "24: this must be a constant: it is needed before the program runs"
    );
    assert_eq!(in_main("const u8 K;"), "10: the constant `K` needs a value");
    assert_eq!(
        in_main("const u8 t[2] = { 1, 2 };"),
        "10: a constant array is kept in ROM; declare it outside the functions"
    );
    assert_eq!(
        in_program("const u8 K; void main() { }"),
        "1:10: the constant `K` needs a value"
    );
    assert_eq!(
        in_program("u8 a = 1; u8 b = a; void main() { }"),
        "1:18: this must be a constant: it is needed before the program runs"
    );
    assert_eq!(
        in_program("const u8 t[2]; void main() { }"),
        "1:10: the constant `t` needs its values"
    );
    assert_eq!(
        in_program("u16 t[2] = \"ab\"; void main() { }"),
        "1:12: a string can only fill an array of `u8`"
    );
    assert_eq!(
        in_program("u8 t[2] = \"abc\"; void main() { }"),
        "1:4: `t` has room for 2 values and 4 are given"
    );
    assert_eq!(
        in_program("u8 t[2] = 5; void main() { }"),
        "1:11: an array is given its values as a list: `{ 1, 2, 3 }`"
    );
    assert_eq!(
        in_program("u8 x = { 1 }; void main() { }"),
        "1:8: only an array can be given a list of values"
    );
}

#[test]
fn statements_in_the_wrong_place() {
    assert_eq!(in_main("break;"), "1: `break` is only for inside a loop");
    assert_eq!(
        in_main("continue;"),
        "1: `continue` is only for inside a loop"
    );
    assert_eq!(in_main("return 1;"), "8: this function returns nothing");
    assert_eq!(
        in_program("u8 f() { return; } void main() { }"),
        "1:10: this function must return a `u8`"
    );
}

#[test]
fn calls() {
    assert_eq!(
        in_main("putc();"),
        "1: `putc` takes 1 argument and 0 are given"
    );
    assert_eq!(
        in_main("putc(1, 2);"),
        "1: `putc` takes 1 argument and 2 are given"
    );
    assert_eq!(
        in_main("print(65);"),
        "7: this is a number and a `u8*` is needed; write `(u8*)…` if that is meant"
    );
    // A function that is promised and never written is found by the
    // assembler, which only knows it by the name the compiler gave it.
    assert_eq!(
        in_program("u8 promised(u8 a);\nvoid main() { promised(1); }"),
        "2:15: `_promised` is not defined"
    );
}

#[test]
fn the_program_as_a_whole() {
    assert_eq!(
        in_program("u8 x;"),
        "1:1: there is no `main` function; a program starts there"
    );
    assert_eq!(
        in_program("void main(u8 a) { }"),
        "1:6: `main` takes no arguments: nobody is there to give them"
    );
    assert_eq!(
        in_program("#include \"missing.h\"\nvoid main() { }"),
        "1:1: cannot read `missing.h`: no such file"
    );
    assert_eq!(
        in_program("void main() {\n    asm { bogus a, b }\n}"),
        "2:11: unknown instruction `bogus`"
    );
    assert_eq!(
        in_program("void main() {\n    asm {\n        ld a, 1\n        bogus\n    }\n}"),
        "4:9: unknown instruction `bogus`"
    );
}

#[test]
fn a_function_that_returns_a_value_must_return_it_on_every_path() {
    let missing = |body: &str| {
        let source = format!("u8 g;\nu8 f(u8 a) {{\n{body}\n}}\nvoid main() {{ f(1); }}");
        errors(&source)
    };
    let complaint =
        "main.c:4:1: `f` must return a `u8`, and its end can be reached without a `return`";
    for body in [
        "",
        "g = a;",
        "if (a) return 1;",
        "if (a) return 1; else g = 2;",
        "while (a) return 1;",
        "while (1) { if (a) break; return 1; }",
        "for (;;) { while (1) break; break; }",
        "do { return 1; } while (0); g = 1;",
        "{ { if (a) { return 1; } } }",
    ] {
        assert_eq!(missing(body), [complaint], "{body}");
    }
    for body in [
        "return a;",
        "if (a) return 1; else return 2;",
        "if (a) { return 1; } else if (g) { return 2; } else { return 3; }",
        "while (1) { if (a) return 1; g++; }",
        "for (;;) { while (g) { break; } if (a) return 2; }",
        "do { g++; if (g > a) return g; } while (1);",
        "{ g = 1; { return g; } }",
        // Assembly at the end is trusted to have left the value in `a`.
        "asm { ld a, 7 }",
    ] {
        let source = format!("u8 g;\nu8 f(u8 a) {{\n{body}\n}}\nvoid main() {{ f(1); }}");
        common::run(&source);
    }
}

#[test]
fn a_frame_larger_than_sp_can_reach_is_refused() {
    assert_eq!(
        in_program("void main() {\n    u8 big[300];\n    big[0] = 1;\n    u8 after = 2;\n}"),
        "4:8: the variables of this function take more room than can be reached from the \
         stack pointer (255 bytes); make the large ones global"
    );
}

#[test]
fn every_error_is_reported_not_just_the_first() {
    let all = errors(
        "void main() {\n    u8 a = 300;\n    b = 1;\n    u8* p = a;\n    a = f(2);\n    break;\n}",
    );
    assert_eq!(
        all,
        [
            "main.c:2:12: 300 does not fit in a `u8`",
            "main.c:3:5: `b` is not declared",
            "main.c:4:13: this is a `u8` and a `u8*` is needed; write `(u8*)…` if that is meant",
            "main.c:5:9: `f` is not declared",
            "main.c:6:5: `break` is only for inside a loop",
        ]
    );
}

#[test]
fn errors_in_an_included_file_name_that_file() {
    let files: std::collections::BTreeMap<String, String> = [
        (
            "main.c",
            "#include \"lib/draw.h\"\nvoid main() { plot(1); }",
        ),
        ("lib/draw.h", "// drawing\nvoid plot(u8 x) {\n    y = x;\n}"),
    ]
    .into_iter()
    .map(|(name, text)| (name.to_string(), text.to_string()))
    .collect();
    let all = silicon_cc::build("main.c", &files).unwrap_err();
    assert_eq!(all.len(), 1);
    assert_eq!(
        all[0].to_string(),
        "lib/draw.h:3:5: error: `y` is not declared"
    );
}
