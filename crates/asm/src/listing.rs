//! The listing: every source line next to the address and bytes it became.

use crate::Assembly;
use std::fmt::Write;

/// Bytes shown per row; a line with more continues on the rows below it.
const BYTES_PER_ROW: usize = 4;

impl Assembly {
    /// The program as a listing: address, bytes, source text.
    ///
    /// ```
    /// let program = silicon_asm::assemble_source("a.asm", "start: ld x, 0x8000\n hlt").unwrap();
    /// assert_eq!(program.listing(), "\
    /// 0000  19 00 80     start: ld x, 0x8000
    /// 0003  00            hlt
    /// ");
    /// ```
    pub fn listing(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            let Some(addr) = line.addr else {
                let _ = writeln!(out, "{:19}{}", "", line.text);
                continue;
            };
            let start = addr as usize;
            // A line outside the ROM (RAM variables) has no bytes.
            let bytes = self
                .rom
                .get(start..start + line.len as usize)
                .unwrap_or(&[]);
            let mut rows = bytes.chunks(BYTES_PER_ROW);
            let _ = writeln!(
                out,
                "{addr:04x}  {:<13}{}",
                hex(rows.next().unwrap_or(&[])),
                line.text
            );
            for (i, row) in rows.enumerate() {
                let at = start + (i + 1) * BYTES_PER_ROW;
                let _ = writeln!(out, "{at:04x}  {}", hex(row));
            }
        }
        // Blank source lines should not leave trailing spaces behind.
        out.lines()
            .map(|l| l.trim_end().to_string() + "\n")
            .collect()
    }
}

fn hex(bytes: &[u8]) -> String {
    let parts: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
    parts.join(" ")
}
