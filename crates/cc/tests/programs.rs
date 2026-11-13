//! Whole programs: compiled, run on the computer, judged by what they print.

mod common;

use common::{output, run};

#[test]
fn hello() {
    let source = r#"
        #include "test.h"
        void main() {
            print("Hello!\n");
            show(12345);
            show_i(-42);
        }
    "#;
    assert_eq!(output(source), "Hello!\n12345 -42 ");
}

#[test]
fn a_sum_in_a_loop() {
    let run = run("u8 total; void main() { for (u8 i = 1; i <= 10; i++) total += i; }");
    assert_eq!(run.byte("_total"), 55);
}
