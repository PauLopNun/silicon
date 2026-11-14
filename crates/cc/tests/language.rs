//! The language, feature by feature: each test is a small program and what
//! it must print.

mod common;

use common::{output, run};

/// A program whose `main` is these statements, with `test.h` at hand.
fn main_of(body: &str) -> String {
    format!("#include \"test.h\"\nvoid main() {{\n{body}\n}}")
}

/// What the statements print.
fn prints(body: &str) -> String {
    output(&main_of(body))
}

#[test]
fn the_four_integer_types_wrap_at_their_width() {
    let out = prints(
        "u8 a = 250; a += 10; show(a);
         i8 b = 120; b += 10; show_i(b);
         u16 c = 65530; c += 10; show(c);
         i16 d = 32760; d += 10; show_i(d);
         u8 e = 3; e -= 5; show(e);
         i16 f = -32768; f -= 1; show_i(f);",
    );
    assert_eq!(out, "4 -126 4 -32766 254 32767 ");
}

#[test]
fn arithmetic_is_done_in_the_wider_type() {
    let out = prints(
        "u8 a = 200; u8 b = 100;
         show(a + b);                   // 8 bits: wraps
         show((u16)a + b);              // 16 bits
         u16 w = a + b; show(w);        // still 8 bits: the sum is made first
         u16 big = 1000; show(big + a);
         i8 n = -1;
         show(big + n);                 // the i8 is widened with its sign
         i16 s = -5; show_i(s * n);
         show(a * 2); show((u16)a * 2); show(a + 300);",
    );
    assert_eq!(out, "44 300 44 1200 999 5 144 400 500 ");
}

#[test]
fn signed_and_unsigned_differ_where_they_should() {
    let out = prints(
        "i8 a = -100; u8 b = 156;      // the same bits
         show(a < 0); show(b < 0);
         show(a < 100); show(b < 100);
         show_i(a / 3); show(b / 3);
         show_i(a % 7); show(b % 7);
         show_i(a >> 2); show(b >> 2);
         i16 c = -1000; u16 d = 64536;
         show(c < 1); show(d < 1);
         show_i(c / 7); show(d / 7);
         show_i(c % 7); show(d % 7);
         show_i(c >> 3); show(d >> 3);
         show_i(-7 / 2); show_i(-7 % 2); show_i(7 / -2); show_i(7 % -2);",
    );
    assert_eq!(
        out,
        "1 0 1 0 -33 52 -2 2 -25 39 1 0 -142 9219 -6 3 -125 8067 -3 -1 -3 1 "
    );
}

#[test]
fn comparisons_of_every_kind() {
    let out = prints(
        "u8 a = 5; u8 b = 200; i8 n = -3; u16 w = 300; i16 s = -300;
         show(a < b); show(a <= b); show(a > b); show(a >= b); show(a == b); show(a != b);
         show(b < a); show(b <= a); show(b > a); show(b >= a);
         show(a <= 5); show(a >= 5); show(a < 5); show(a > 5); show(5 < a); show(4 < a);
         show(n < 0); show(n > -4); show(n >= -3); show(n <= -4);
         show(w > 255); show(w == 300); show(w < 301); show(w >= 301);
         show(s < 0); show(s < -300); show(s <= -300); show(s > -301); show(s == -300);
         show(w > a); show(s < n);",
    );
    assert_eq!(
        out,
        "1 1 0 0 0 1 0 0 1 1 1 1 0 0 0 1 1 1 1 0 1 1 1 0 1 0 1 1 1 1 1 "
    );
}

#[test]
fn sixteen_bit_comparisons_look_at_both_bytes() {
    let out = prints(
        "u16 a = 0x0100; u16 b = 0x00ff; u16 c = 0x0200; u16 d = 0x0100;
         show(a > b); show(a < b); show(a < c); show(a == d); show(a != d); show(a >= d);
         show(a == 0x0100); show(a == 0x0101); show(a == 0x0200); show(a == 0);
         i16 m = -1; i16 z = 0; i16 p = 256;
         show(m < z); show(z < m); show(m < p); show(p < m); show(m < -1); show(m <= -1);
         if (a) show(1); if (!a) show(2); u16 zero = 0; if (zero) show(3); if (!zero) show(4);",
    );
    assert_eq!(out, "1 0 1 1 0 1 1 0 0 0 1 0 1 0 0 1 1 4 ");
}

#[test]
fn logic_stops_as_soon_as_it_knows() {
    let source = r#"
        #include "test.h"
        u8 calls;
        u8 yes() { calls++; return 1; }
        u8 no() { calls++; return 0; }
        void main() {
            show(no() && yes()); show(calls);
            show(yes() || no()); show(calls);
            show(yes() && no()); show(calls);
            show(no() || yes()); show(calls);
            show(!no()); show(!yes());
            show(yes() && yes() && no() || yes());
            u8 a = 3; u16 w = 0x100;
            show(a && w); show(!w); show(!!a); show(a == 3 && w != 0 || a > 9);
            if (a > 1 && a < 5) print("in "); else print("out ");
            if (a < 1 || a > 5) print("out "); else print("in ");
            if (!(a == 3)) print("no "); else print("yes ");
        }
    "#;
    assert_eq!(output(source), "0 1 1 2 0 4 1 6 1 0 1 1 0 1 1 in in yes ");
}

#[test]
fn bit_operations_and_shifts() {
    let out = prints(
        "u8 a = 0b1100_1010 & 0xff; u8 n = 3; u16 w = 0xa5f0; i16 s = -256; i8 t = -128;
         show(a & 0x0f); show(a | 0x0f); show(a ^ 0xff); show(~a & 0xff);
         show(a << 1); show(a >> 1); show(a << n); show(a >> n); show(a << 8); show(a >> 8);
         show(w & 0x0ff0); show(w | 0x000f); show(w ^ 0xffff); show(~w);
         show(w << 1); show(w >> 1); show(w << n); show(w >> n);
         show(w << 8); show(w >> 8); show(w << 12); show(w >> 12); show(w << 16); show(w >> 16);
         show_i(s >> 1); show_i(s >> n); show_i(s >> 8); show_i(s >> 12); show_i(s >> 15);
         show_i(s >> 16); show_i(s << 2);
         show_i(t >> 1); show_i(t >> 7); show_i(t >> 9); show_i(t >> n);
         u8 big = 20; show(w << big); show(w >> big); show_i(s >> big); show(a << big);",
    );
    assert_eq!(
        out,
        "10 207 53 53 148 101 80 25 0 0 \
         1520 42495 23055 23055 \
         19424 21240 12160 5310 \
         61440 165 0 10 0 0 \
         -128 -32 -1 -1 -1 -1 -1024 \
         -64 -1 -1 -16 \
         0 0 -1 0 "
    );
}

#[test]
fn digits_may_be_grouped_as_in_the_assembler() {
    assert_eq!(
        prints("show(1_000); show(0x1_00); show(0b1_0000_0000);"),
        "1000 256 256 "
    );
}

#[test]
fn multiplying_and_dividing() {
    let out = prints(
        "u8 a = 13; u8 b = 17; u16 w = 1234; u16 v = 53; i16 s = -77; i8 n = -5;
         show(a * b); show((u16)a * b); show(w * v); show(w / v); show(w % v);
         show(a / b); show(b / a); show(b % a); show(a * 4); show(a / 4); show(a % 4);
         show(w * 8); show(w / 8); show(w % 8); show(w * 256); show(w / 256); show(w % 256);
         show_i(s * n); show_i(s / n); show_i(s % n); show_i(s * 3); show_i(s / 3); show_i(s % 3);
         show_i(n * n); show_i(n / 2); show_i(n % 2); show_i(n * a);
         u8 zero = 0; u16 zero16 = 0;
         show(a / zero); show(a % zero); show(w / zero16); show(w % zero16);
         show(a * zero); show(w * zero16); show(0 * a);",
    );
    assert_eq!(
        out,
        "221 221 65402 23 15 0 1 4 52 3 1 \
         9872 154 2 53760 4 210 \
         385 15 -2 -231 -25 -2 \
         25 -2 -1 191 \
         255 13 65535 1234 0 0 0 "
    );
}

#[test]
fn negation_and_inversion() {
    let out = prints(
        "u8 a = 5; i8 n = -128; u16 w = 1; i16 s = -32768; i16 t = 300;
         show(-a); show_i(-n); show(-w); show_i(-s); show_i(-t); show_i(-(-t));
         show(~a); show_i(~n); show(~w); show_i(~s); show_i(~t);
         show_i(-(t + 1)); show(-(u16)a);",
    );
    assert_eq!(
        out,
        "251 -128 65535 -32768 -300 300 250 127 65534 32767 -301 -301 65531 "
    );
}

#[test]
fn steps_before_and_after() {
    let out = prints(
        "u8 a = 5; u16 w = 255; i8 n = -1;
         show(a++); show(a); show(++a); show(a--); show(--a); show(a);
         show(w++); show(w); show(++w); show(w--); show(--w);
         show_i(n++); show_i(n); show_i(--n); show_i(--n);
         a = 255; a++; show(a); a--; show(a);
         w = 0xffff; ++w; show(w); --w; show(w);",
    );
    assert_eq!(
        out,
        "5 6 7 7 5 5 255 256 257 257 255 -1 0 -1 -2 0 255 0 65535 "
    );
}

#[test]
fn assignment_is_an_expression_and_goes_right_to_left() {
    let out = prints(
        "u8 a; u8 b; u16 w; i16 s;
         a = b = 7; show(a + b);
         w = a = 200; show(w);
         s = (i8)(a = 255); show_i(s);
         w = (a = 3) + (b = 4); show(w);
         a += b -= 1; show(a); show(b);
         w = 1000; w -= 1; w *= 3; w /= 7; w %= 100; show(w);
         w = 0x00f0; w |= 0x0f00; w &= 0x0ff0; w ^= 0x0110; w <<= 4; w >>= 2; show(w);
         s = -100; s /= 7; s *= -3; s >>= 1; show_i(s);
         a = 9; a <<= 2; a >>= 1; a |= 1; a ^= 0x80; a &= 0xf0; show(a);
         if ((a = 0)) show(1); else show(2);",
    );
    assert_eq!(out, "14 200 -1 7 6 3 28 15232 21 144 2 ");
}

#[test]
fn if_else_and_the_conditional() {
    let out = prints(
        "for (u8 i = 0; i < 5; i++) {
             if (i == 0) print(\"zero \");
             else if (i == 1) print(\"one \");
             else if (i < 4) print(\"some \");
             else print(\"many \");
         }
         u8 a = 3; u16 w = 1000; i8 n = -2;
         show(a > 2 ? 10 : 20); show(a > 5 ? 10 : 20);
         show(a ? w : 5); show(n < 0 ? a : w);
         show_i(a == 3 ? n : 7); show(a == 3 ? a < 9 ? 1 : 2 : 3);
         show(1 ? 4 : 5); show(0 ? 4 : 5);",
    );
    assert_eq!(out, "zero one some some many 10 20 1000 3 -2 1 4 5 ");
}

#[test]
fn every_loop_with_break_and_continue() {
    let out = prints(
        "u8 i = 0;
         while (i < 3) show(i++);
         do show(i--); while (i);
         do { show(9); } while (0);
         for (i = 10; i > 6; i -= 2) show(i);
         for (u8 k = 0; k < 10; k++) {
             if (k == 2) continue;
             if (k == 5) break;
             show(k);
         }
         u8 n = 0;
         for (;;) { if (++n == 4) break; }
         show(n);
         while (1) { n++; if (n < 8) continue; break; }
         show(n);
         do { n++; if (n & 1) continue; show(n); } while (n < 12);
         for (u8 r = 0; r < 3; r++) {
             for (u8 c = 0; c < 3; c++) {
                 if (c > r) break;
                 putc('a' + r * 3 + c);
             }
         }
         u16 total = 0;
         for (u16 w = 1; w <= 300; w++) total += w;
         show(total);",
    );
    assert_eq!(out, "0 1 2 3 2 1 9 10 8 0 1 3 4 4 8 10 12 adeghi45150 ");
}

#[test]
fn functions_take_arguments_and_return_values() {
    let source = r#"
        #include "test.h"
        u8 add8(u8 a, u8 b) { return a + b; }
        u16 add16(u16 a, u16 b) { return a + b; }
        i16 neg(i16 a) { return -a; }
        u16 mix(u8 a, u16 b, i8 c, i16 d) { return a + b + c + d; }
        u8 first(u8 a, u8 b, u8 c) { return a; }
        u8 last(u8 a, u8 b, u8 c) { return c; }
        void nothing() { }
        void early(u8 a) { if (a) return; print("late "); }
        u16 wide(u8 a) { return a; }
        u8 narrow(u16 a) { return a; }
        i16 sign(i8 a) { return a; }
        u16 later(u8 n);
        void main() {
            show(add8(200, 100)); show(add16(200, 100)); show_i(neg(5)); show_i(neg(-32768));
            show(mix(1, 1000, -2, -500));
            show(first(1, 2, 3)); show(last(1, 2, 3));
            nothing(); early(1); early(0);
            show(wide(255)); show(narrow(0x1234)); show_i(sign(-7));
            show(add8(add8(1, 2), add8(3, add8(4, 5))));
            show(add16(add8(250, 10), 1) + add16(1, add16(2, 3)) * 2);
            show(later(4));
        }
        u16 later(u8 n) { return (u16)n * 100; }
    "#;
    assert_eq!(
        output(source),
        "44 300 -5 -32768 499 1 3 late 255 52 -7 15 17 400 "
    );
}

#[test]
fn recursion() {
    let source = r#"
        #include "test.h"
        u16 fib(u8 n) { if (n < 2) return n; return fib(n - 1) + fib(n - 2); }
        u16 fact(u16 n) { return n ? n * fact(n - 1) : 1; }
        u8 even(u8 n);
        u8 odd(u8 n) { return n ? even(n - 1) : 0; }
        u8 even(u8 n) { return n ? odd(n - 1) : 1; }
        u16 gcd(u16 a, u16 b) { return b ? gcd(b, a % b) : a; }
        u16 depth(u16 n) { u16 here = n; if (n == 0) return 0; return depth(n - 1) + here; }
        void main() {
            show(fib(20)); show(fact(8)); show(even(10)); show(odd(10)); show(gcd(1071, 462));
            show(depth(200));
        }
    "#;
    assert_eq!(output(source), "6765 40320 1 0 21 20100 ");
}

#[test]
fn locals_in_blocks_shadow_and_give_their_room_back() {
    let source = r#"
        #include "test.h"
        u8 x = 1;
        void main() {
            show(x);
            u8 x = 2;
            show(x);
            {
                u8 x = x + 1;       // the x outside, plus one
                show(x);
                { u16 x = 500; show(x); }
                show(x);
            }
            show(x);
            for (u8 x = 7; x < 9; x++) show(x);
            show(x);
            { u16 a = 1; u16 b = 2; show(a + b); }
            { u8 c = 3; u8 d = 4; u16 e = 5; show(c + d + e); }
        }
    "#;
    let run = run(source);
    assert_eq!(run.output(), "1 2 3 500 3 2 7 8 2 3 12 ");
    assert_eq!(run.byte("_x"), 1, "the global was never touched");
}

#[test]
fn globals_start_with_their_values_or_with_zero() {
    let source = r#"
        #include "test.h"
        const u8 N = 4;
        const u16 BIG = N * 1000 + 1;
        u8 a = 7; i8 b = -7; u16 c = 0xbeef; i16 d = -2;
        u8 zero; u16 zero16;
        u8 small[N] = { 1, 2 };
        u16 words[] = { 1, 256, BIG, };
        i8 neg[2] = { -1, -128 };
        const u8 rom[] = { 9, 8, 7 };
        const u16 rom16[2] = { 0x1234, 0xffff };
        u8 text[] = "abc";
        u8 exact[3] = "xyz";
        const u8 message[] = "in rom";
        u8* pointer = small + 1;
        u8* string = "a string";
        u16* wp = &words[2];
        void main() {
            show(N); show(BIG); show(a); show_i(b); show(c); show_i(d); show(zero); show(zero16);
            show(small[0]); show(small[1]); show(small[2]); show(small[3]);
            show(words[0]); show(words[1]); show(words[2]);
            show_i(neg[0]); show_i(neg[1]);
            show(rom[0] + rom[1] + rom[2]); show(rom16[0]); show(rom16[1]);
            print(text); putc(' '); putc(exact[0]); putc(exact[2]); putc(' ');
            print(message); putc(' ');
            show(*pointer); print(string); putc(' '); show(*wp);
            show(sizeof small); show(sizeof words); show(sizeof text); show(sizeof exact);
            show(sizeof(u16)); show(sizeof(u8*)); show(sizeof a); show(sizeof c); show(sizeof rom16);
        }
    "#;
    let run = run(source);
    assert_eq!(
        run.output(),
        "4 4001 7 -7 48879 -2 0 0 1 2 0 0 1 256 4001 -1 -128 24 4660 65535 \
         abc xz in rom 2 a string 4001 4 6 4 3 2 2 1 2 4 "
    );
    // Constants are in ROM; the rest in RAM.
    assert!(run.program.symbols["_rom"] < 0x8000);
    assert!(run.program.symbols["_message"] < 0x8000);
    assert!(run.program.symbols["_small"] >= 0x9000);
    assert!(
        !run.program.symbols.contains_key("_N"),
        "a constant takes no room"
    );
}

#[test]
fn constants_are_names_for_values_inside_functions_too() {
    let source = r#"
        #include "test.h"
        const u8 N = 4;
        u8 table[N * 2];
        void main() {
            const u16 BIG = N * 1000;
            const u8* START = table + 1;
            show(BIG + 1); show(sizeof table); show(sizeof BIG);
            {
                const u8 N = 9;         // only in this block
                u8 inner[N]; show(sizeof inner); show(N);
            }
            show(N);
            *START = N; show(table[1]);
        }
    "#;
    let run = run(source);
    assert_eq!(run.output(), "4001 8 2 9 9 4 4 ");
    // No room is set aside for a constant: `main` needs none of its own
    // outside the inner block's array.
    assert!(!common::assembly(source).contains("BIG"));
}

#[test]
fn pointers_and_arrays() {
    let source = r#"
        #include "test.h"
        u8 bytes[8];
        u16 words[4];
        u8 grid[4 * 4];
        void fill(u8* p, u8 n, u8 value) { while (n--) *p++ = value++; }
        u16 sum(u8* p, u8 n) { u16 total = 0; for (u8 i = 0; i < n; i++) total += p[i]; return total; }
        void swap(u16* a, u16* b) { u16 t = *a; *a = *b; *b = t; }
        u8 length(u8* s) { u8 n = 0; while (s[n]) n++; return n; }
        void main() {
            fill(bytes, 8, 10);
            show(sum(bytes, 8)); show(bytes[0]); show(bytes[7]);
            u8* p = bytes + 2;
            show(*p); show(p[1]); show(*(p + 2)); show(p[-1]); show(*(p - 2));
            p += 3; show(*p); p -= 1; show(*p); p++; show(*p); --p; show(*p);
            *p = 99; show(bytes[4]); p[1] = 98; show(bytes[5]);
            (*p)++; show(bytes[4]); *p += 10; show(bytes[4]); p[1] -= 8; show(bytes[5]);
            for (u8 i = 0; i < 4; i++) words[i] = 1000 * (i + 1);
            u16* w = words;
            show(*w); show(w[3]); show(*(w + 1)); w++; show(*w); w += 2; show(*w); show(w[-1]);
            swap(&words[0], &words[3]); show(words[0]); show(words[3]);
            swap(words + 1, w); show(words[1]); show(*w);
            *w = *w + 1; show(words[3]); w[-3] += 5; show(words[0]); words[1]++; show(words[1]);
            u16 local = 7; u16* lp = &local; *lp += 1; show(local); swap(lp, words); show(local);
            u8 buffer[5]; fill(buffer, 5, 1); show(sum(buffer, 5)); show(sizeof buffer);
            u8 row = 2; u8 col = 3; grid[row * 4 + col] = 42; show(grid[11]); show(sum(grid, 16));
            show(length("hello")); show(length("")); show("abc"[1]);
            u8* q = (u8*)words; show(*q + q[1] * 256);
            show((u16)(w - 1) - (u16)words); show(w == words + 3); show(w != 0); show(!w);
            u16 big = 300; bytes[0] = 1; show(bytes[big - 300]); show(grid[big - 289]);
            i8 back = -1; show(p[back]); i16 back16 = -2; show(p[back16]);
        }
    "#;
    assert_eq!(
        output(source),
        "108 10 17 12 13 14 11 10 15 14 15 14 99 98 100 110 90 \
         1000 4000 2000 2000 4000 3000 4000 1000 1000 2000 2001 4005 1001 \
         8 4005 15 5 42 42 5 0 98 8 4 1 1 0 1 42 13 12 "
    );
}

#[test]
fn memory_can_be_reached_by_address() {
    let source = r#"
        #include "test.h"
        const u16 SCREEN = 0x8000;
        void main() {
            *(u8*)0x8000 = 0xe4;
            *(u8*)(SCREEN + 2047) = 0x1b;
            u8* screen = (u8*)SCREEN;
            screen[32] = screen[0] + 1;
            *(u16*)0x9100 = 0x1234;
            show(*(u8*)0x9100); show(*(u8*)0x9101); show(*(u16*)0x9100);
            u16 where = 0x9100;
            show(*(u16*)where); *(u8*)(where + 1) = 0; show(*(u16*)where);
            show(*(u8*)0x8800);
        }
    "#;
    let run = run(source);
    assert_eq!(run.output(), "52 18 4660 4660 52 0 ");
    let screen = run.machine.board().framebuffer();
    assert_eq!((screen[0], screen[32], screen[2047]), (0xe4, 0xe5, 0x1b));
}

#[test]
fn casts() {
    let out = prints(
        "u16 w = 0x1234; i8 n = -2; u8 a = 254; i16 s = -2;
         show((u8)w); show((u16)n); show((u16)(u8)n); show_i((i8)a); show_i((i16)a);
         show_i((i16)(i8)a); show((u8)s); show((u16)s); show((u8)300); show_i((i8)200);
         show((u16)a + 1); show((u8)(a + 2)); show_i((i8)w); show((u16)(u8*)0x1234);
         show((u8)(u8*)0x1234);",
    );
    assert_eq!(
        out,
        "52 65534 254 -2 254 -2 254 65534 44 -56 255 0 52 4660 52 "
    );
}

#[test]
fn assembly_inside_a_program() {
    let source = r#"
        #include "test.h"
        u8 counter;
        u8 twice(u8 n);
        u16 pair(u8 lo, u8 hi);
        asm {
        ; A function written in assembly: arguments above the return address,
        ; the first one farthest; result in a (or ba).
        _twice: ld   a, [sp+2]
                add  a, a
                ret
        _pair:  ld   a, [sp+4]
                ld   b, [sp+2]
                ret
        }
        void main() {
            show(twice(21)); show(pair(0x34, 0x12));
            asm {
                ld   a, [_counter]
                add  a, 5
                st   [_counter], a
            }
            show(counter);
            for (u8 i = 0; i < 3; i++) asm { ld a, [_counter]
                inc a
                st [_counter], a
            }
            show(counter);
        }
    "#;
    assert_eq!(output(source), "42 4660 5 8 ");
}

#[test]
fn local_labels_of_assembly_belong_to_the_function_they_are_in() {
    // The compiler puts labels of its own between the two blocks (for the
    // `while` and the `if`); the second block still finds `.again`.
    let source = r#"
        #include "test.h"
        u8 n;
        void count(u8 from) {
            n = from;
            asm {
        .again: ld   a, [_n]
                inc  a
                st   [_n], a
            }
            while (n & 3) n++;
            if (n < 20) asm { jmp .again }
        }
        void other() { asm {
        .again: ld   a, 1
            }
        }
        void main() { count(1); show(n); other(); }
    "#;
    assert_eq!(output(source), "20 ");
}

#[test]
fn many_arguments_and_deep_expressions_keep_the_stack_straight() {
    let source = r#"
        #include "test.h"
        u16 six(u16 a, u8 b, u16 c, u8 d, u16 e, u8 f) { return a - b + c - d + e - f; }
        u16 id(u16 x) { return x; }
        void main() {
            u8 a = 1; u16 b = 2; u8 c = 3; u16 d = 4;
            show(six(100, a, id(200) + id(b), c, id(id(300)) * id(d), id(5)));
            show((id(1) + (id(2) + (id(3) + (id(4) + (id(5) + id(6)))))) * (id(a) + id(b)));
            show(id(a + id(b + id(c + id(d)))) - (id(d) - (id(c) - (id(b) - id(a)))));
            u16 table[3]; table[id(1)] = id(7) * id(6); table[0] = table[id(1)] + id(1);
            show(table[0]); show(table[1]);
            *(table + id(2)) = id(table[0]) + id(table[1]); show(table[2]);
        }
    "#;
    assert_eq!(output(source), "1493 63 8 43 42 85 ");
}

#[test]
fn large_frames_and_many_locals() {
    let source = r#"
        #include "test.h"
        u16 f(u16 seed) {
            u8 buffer[200];
            u16 a = seed; u16 b = seed + 1; u16 c = seed + 2;
            for (u8 i = 0; i < 200; i++) buffer[i] = i;
            u16 total = 0;
            for (u8 i = 0; i < 200; i++) total += buffer[i];
            return total + a + b + c;
        }
        void main() { show(f(10)); }
    "#;
    assert_eq!(output(source), "19933 ");
}

#[test]
fn strings_and_characters() {
    let out = prints(
        r#"print("tab\there\n"); print("quote\" back\\ hex\x41\x7a zero-ends\0hidden");
           putc('\n'); putc('\''); putc('A' + 2); show('a'); show('\n'); show('\0');
           print("two " "pieces"); u8* s = "xyz"; show(s[0] + s[1] + s[2]); show(s[3]);"#,
    );
    assert_eq!(
        out,
        "tab\there\nquote\" back\\ hexAz zero-ends\n'C97 10 0 two pieces107 0 "
    );
}

#[test]
fn main_may_return_early_and_the_program_still_stops() {
    let source = "u8 reached; void main() { reached = 1; if (reached) return; reached = 2; }";
    assert_eq!(run(source).byte("_reached"), 1);
}

#[test]
fn the_same_program_runs_the_same_on_the_gates() {
    use silicon_machine::EngineKind;
    let source = r#"
        #include "test.h"
        u16 fib(u8 n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }
        i16 table[4] = { -300, 7, 1000, -1 };
        void main() {
            show(fib(10));
            i16 total = 0;
            for (u8 i = 0; i < 4; i++) total += table[i] * (i + 1) / 3 % 50 << 1;
            show_i(total);
        }
    "#;
    let fast = common::run_on(source, EngineKind::Fast, 2_000_000);
    let gate = common::run_on(source, EngineKind::Gate, 2_000_000);
    assert_eq!(fast.output(), gate.output());
    assert_eq!(fast.cycles, gate.cycles);
    assert_eq!(fast.output(), "55 6 ");
}
