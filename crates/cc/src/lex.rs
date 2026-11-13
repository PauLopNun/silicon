//! Source text → tokens, following `#include`.

use silicon_asm::{resolve_include, Diagnostic, Files, Pos};

/// A place in the source: which file, and where in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct Loc {
    /// Index into the list of file names.
    pub file: usize,
    pub line: u32,
    pub col: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Tok {
    /// A name or a keyword.
    Ident(String),
    Num(i64),
    Str(Vec<u8>),
    /// An operator or a punctuation mark, e.g. `"<<="`.
    Punct(&'static str),
    /// The inside of an `asm { … }` block, untouched. Its lines are the
    /// lines of the source, the first one being line `Loc::line` of the
    /// token, padded so that columns are the same too.
    Asm(String),
    Eof,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Token {
    pub tok: Tok,
    pub loc: Loc,
}

/// Longest first, so that `<<=` is not read as `<` `<=`.
const PUNCTUATION: [&str; 44] = [
    "<<=", ">>=", "==", "!=", "<=", ">=", "&&", "||", "<<", ">>", "++", "--", "+=", "-=", "*=",
    "/=", "%=", "&=", "|=", "^=", "+", "-", "*", "/", "%", "&", "|", "^", "~", "!", "<", ">", "=",
    "?", ":", ";", ",", "(", ")", "{", "}", "[", "]", ".",
];

/// How deep `#include` may nest.
const MAX_DEPTH: usize = 16;

pub(crate) struct Lexer<'a> {
    files: &'a dyn Files,
    /// Every file read so far. A file is only ever read once: including it
    /// again does nothing, so headers need no guards.
    pub names: Vec<String>,
    pub tokens: Vec<Token>,
}

type Result<T> = std::result::Result<T, Diagnostic>;

impl<'a> Lexer<'a> {
    pub fn new(files: &'a dyn Files) -> Self {
        Lexer {
            files,
            names: Vec::new(),
            tokens: Vec::new(),
        }
    }

    /// Read `main` and everything it includes; ends with [`Tok::Eof`].
    pub fn run(mut self, main: &str) -> Result<(Vec<Token>, Vec<String>)> {
        let text = self.files.read(main).map_err(|why| Diagnostic {
            file: main.to_string(),
            pos: Pos { line: 1, col: 1 },
            message: format!("cannot read `{main}`: {why}"),
        })?;
        self.names.push(main.to_string());
        self.file(0, &text, 0)?;
        let last = self.tokens.last().map_or(Loc::default(), |t| t.loc);
        self.tokens.push(Token {
            tok: Tok::Eof,
            loc: last,
        });
        Ok((self.tokens, self.names))
    }

    fn error(&self, loc: Loc, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            file: self.names[loc.file].clone(),
            pos: Pos {
                line: loc.line,
                col: loc.col,
            },
            message: message.into(),
        }
    }

    fn file(&mut self, file: usize, text: &str, depth: usize) -> Result<()> {
        let lines: Vec<Vec<char>> = text.lines().map(|l| l.chars().collect()).collect();
        let mut in_comment = false;
        // The place being read, and whether it is the start of a line.
        let (mut row, mut i, mut fresh) = (0, 0, true);
        while row < lines.len() {
            let line = &lines[row];
            let at = Loc {
                file,
                line: row as u32 + 1,
                col: i as u32 + 1,
            };
            // `#include "file"` has a line to itself.
            let first = line.iter().position(|c| !c.is_whitespace());
            if fresh && !in_comment && first.is_some_and(|f| line[f] == '#') {
                let at = Loc {
                    col: first.unwrap_or(0) as u32 + 1,
                    ..at
                };
                self.include(file, at, line, depth)?;
                row += 1;
                continue;
            }
            fresh = false;
            if i >= line.len() {
                (row, i, fresh) = (row + 1, 0, true);
                continue;
            }
            let c = line[i];
            let next = line.get(i + 1).copied();
            if in_comment {
                if c == '*' && next == Some('/') {
                    in_comment = false;
                    i += 1;
                }
                i += 1;
                continue;
            }
            let tok = if c.is_whitespace() {
                i += 1;
                continue;
            } else if c == '/' && next == Some('/') {
                i = line.len();
                continue;
            } else if c == '/' && next == Some('*') {
                in_comment = true;
                i += 2;
                continue;
            } else if c.is_ascii_digit() {
                let start = i;
                while i < line.len() && (line[i].is_ascii_alphanumeric() || line[i] == '_') {
                    i += 1;
                }
                let text: String = line[start..i].iter().collect();
                match number(&text) {
                    Some(n) => Tok::Num(n),
                    None => return Err(self.error(at, format!("`{text}` is not a number"))),
                }
            } else if c.is_ascii_alphabetic() || c == '_' {
                let start = i;
                while i < line.len() && (line[i].is_ascii_alphanumeric() || line[i] == '_') {
                    i += 1;
                }
                let word: String = line[start..i].iter().collect();
                let brace = line[i..].iter().position(|c| !c.is_whitespace());
                if word == "asm" && brace.is_some_and(|b| line[i + b] == '{') {
                    let open = i + brace.unwrap_or(0);
                    let (text, end_row, end_col) = self.asm_block(&lines, row, open, at)?;
                    // Carry on just past the closing brace.
                    (row, i) = (end_row, end_col);
                    Tok::Asm(text)
                } else {
                    Tok::Ident(word)
                }
            } else if c == '\'' {
                i += 1;
                let value = self.character(line, &mut i, at)?;
                if line.get(i) != Some(&'\'') {
                    return Err(self.error(at, "a character needs a closing `'`"));
                }
                i += 1;
                Tok::Num(value as i64)
            } else if c == '"' {
                i += 1;
                let mut bytes = Vec::new();
                loop {
                    match line.get(i) {
                        None => return Err(self.error(at, "a string needs a closing `\"`")),
                        Some('"') => break,
                        Some(_) => bytes.push(self.character(line, &mut i, at)?),
                    }
                }
                i += 1;
                Tok::Str(bytes)
            } else {
                let rest: String = line[i..].iter().take(3).collect();
                match PUNCTUATION.iter().find(|p| rest.starts_with(**p)) {
                    Some(p) => {
                        i += p.len();
                        Tok::Punct(p)
                    }
                    None => return Err(self.error(at, format!("unexpected character `{c}`"))),
                }
            };
            self.tokens.push(Token { tok, loc: at });
        }
        if in_comment {
            let end = Loc {
                file,
                line: lines.len() as u32,
                col: 1,
            };
            return Err(self.error(end, "a `/*` comment is never closed"));
        }
        Ok(())
    }

    /// The text of an `asm { … }` block whose `{` is at `lines[row][open]`.
    /// Returns it with the row and column just past the closing `}`.
    ///
    /// A block ends at the first `}` that begins a line; a block written on
    /// one line ends at the last `}` of that line. (Assembly has no braces
    /// of its own, but its strings and comments may.)
    fn asm_block(
        &self,
        lines: &[Vec<char>],
        row: usize,
        open: usize,
        at: Loc,
    ) -> Result<(String, usize, usize)> {
        let line = &lines[row];
        if let Some(close) = line.iter().rposition(|&c| c == '}').filter(|&c| c > open) {
            let inside: String = line[open + 1..close].iter().collect();
            return Ok((" ".repeat(open + 1) + &inside, row, close + 1));
        }
        let first: String = line[open + 1..].iter().collect();
        let mut text = " ".repeat(open + 1) + &first;
        for (r, line) in lines.iter().enumerate().skip(row + 1) {
            let start = line.iter().position(|c| !c.is_whitespace());
            if let Some(start) = start.filter(|&s| line[s] == '}') {
                return Ok((text, r, start + 1));
            }
            text.push('\n');
            text.extend(line.iter());
        }
        Err(self.error(at, "this `asm {` is never closed by a `}` at the start of a line"))
    }

    fn include(&mut self, from: usize, at: Loc, line: &[char], depth: usize) -> Result<()> {
        let text: String = line.iter().collect();
        let rest = text.trim_start().trim_start_matches('#').trim_start();
        let Some(rest) = rest.strip_prefix("include") else {
            return Err(self.error(at, "the only `#` line there is is `#include \"file\"`"));
        };
        let rest = rest.trim();
        let path = rest
            .strip_prefix('"')
            .and_then(|r| r.strip_suffix('"'))
            .filter(|p| !p.is_empty() && !p.contains('"'));
        let Some(path) = path else {
            return Err(self.error(at, "expected `#include \"file\"`"));
        };
        let path = resolve_include(&self.names[from], path);
        if self.names.contains(&path) {
            return Ok(());
        }
        if depth >= MAX_DEPTH {
            return Err(self.error(at, "includes are nested too deeply"));
        }
        let text = self
            .files
            .read(&path)
            .map_err(|why| self.error(at, format!("cannot read `{path}`: {why}")))?;
        self.names.push(path);
        self.file(self.names.len() - 1, &text, depth + 1)
    }

    /// One character of a string or character literal, handling `\` escapes.
    fn character(&self, line: &[char], i: &mut usize, at: Loc) -> Result<u8> {
        let Some(&c) = line.get(*i) else {
            return Err(self.error(at, "unfinished literal"));
        };
        *i += 1;
        if c != '\\' {
            return u8::try_from(c as u32)
                .ok()
                .filter(u8::is_ascii)
                .ok_or_else(|| self.error(at, format!("`{c}` is not an ASCII character")));
        }
        let escape = line.get(*i).copied();
        *i += 1;
        Ok(match escape {
            Some('n') => b'\n',
            Some('r') => b'\r',
            Some('t') => b'\t',
            Some('0') => 0,
            Some('\\') => b'\\',
            Some('\'') => b'\'',
            Some('"') => b'"',
            Some('x') => {
                let hex: String = line.iter().skip(*i).take(2).collect();
                *i += 2;
                u8::from_str_radix(&hex, 16)
                    .map_err(|_| self.error(at, "`\\x` needs two hexadecimal digits"))?
            }
            other => {
                let shown = other.map(String::from).unwrap_or_default();
                return Err(self.error(at, format!("unknown escape `\\{shown}`")));
            }
        })
    }
}

/// `42`, `0x2a`, `0b101010`.
fn number(text: &str) -> Option<i64> {
    let lower = text.to_ascii_lowercase();
    let value = if let Some(hex) = lower.strip_prefix("0x") {
        i64::from_str_radix(hex, 16).ok()?
    } else if let Some(bin) = lower.strip_prefix("0b") {
        i64::from_str_radix(bin, 2).ok()?
    } else {
        lower.parse().ok()?
    };
    // Nothing in a 16-bit machine is larger; this keeps arithmetic on
    // constants far from overflow.
    (value <= u32::MAX as i64).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(files: &[(&str, &str)]) -> Result<Vec<Token>> {
        let map: std::collections::BTreeMap<String, String> = files
            .iter()
            .map(|(n, t)| (n.to_string(), t.to_string()))
            .collect();
        Lexer::new(&map).run(files[0].0).map(|(tokens, _)| tokens)
    }

    fn toks(source: &str) -> Vec<Tok> {
        let mut all: Vec<Tok> = lex(&[("a.c", source)])
            .unwrap()
            .into_iter()
            .map(|t| t.tok)
            .collect();
        assert_eq!(all.pop(), Some(Tok::Eof));
        all
    }

    #[test]
    fn words_numbers_and_operators() {
        use Tok::*;
        assert_eq!(
            toks("u8 x = a<<=2; // gone\n y >>= 0x1F /* gone\n too */ + 'A' - 0b101;"),
            [
                Ident("u8".into()),
                Ident("x".into()),
                Punct("="),
                Ident("a".into()),
                Punct("<<="),
                Num(2),
                Punct(";"),
                Ident("y".into()),
                Punct(">>="),
                Num(31),
                Punct("+"),
                Num(65),
                Punct("-"),
                Num(5),
                Punct(";"),
            ]
        );
        assert_eq!(toks("a+++b"), [Ident("a".into()), Punct("++"), Punct("+"), Ident("b".into())]);
        assert_eq!(toks(r#""hi\n" "\x41\0""#), [Str(b"hi\n".to_vec()), Str(vec![0x41, 0])]);
    }

    #[test]
    fn positions_are_lines_and_columns() {
        let tokens = lex(&[("a.c", "u8 x;\n\n  x = 1;")]).unwrap();
        let at: Vec<(u32, u32)> = tokens.iter().map(|t| (t.loc.line, t.loc.col)).collect();
        assert_eq!(at, [(1, 1), (1, 4), (1, 5), (3, 3), (3, 5), (3, 7), (3, 8), (3, 8)]);
    }

    #[test]
    fn asm_blocks_are_kept_as_they_are() {
        use Tok::*;
        let source = "void f() {\n    asm {\n        ld a, '}'   ; a } here\n.loop:  jmp .loop\n    } x;\n}";
        assert_eq!(
            toks(source),
            [
                Ident("void".into()),
                Ident("f".into()),
                Punct("("),
                Punct(")"),
                Punct("{"),
                Asm("         \n        ld a, '}'   ; a } here\n.loop:  jmp .loop".into()),
                Ident("x".into()),
                Punct(";"),
                Punct("}"),
            ]
        );
        let tokens = lex(&[("a.c", source)]).unwrap();
        assert_eq!((tokens[5].loc.line, tokens[5].loc.col), (2, 5));
        assert_eq!((tokens[6].loc.line, tokens[6].loc.col), (5, 7), "after the block");
        assert_eq!(tokens[8].loc.line, 6);
        // On one line.
        assert_eq!(
            toks("asm { hlt } y"),
            [Asm("      hlt ".into()), Ident("y".into())]
        );
        assert!(lex(&[("a.c", "asm {\n hlt\n")]).unwrap_err().message.contains("never closed"));
    }

    #[test]
    fn includes_are_read_in_place_and_only_once() {
        let files = [
            ("game/main.c", "#include \"../lib/io.h\"\n#include \"util.h\"\nu8 a;"),
            ("lib/io.h", "u8 b;"),
            ("game/util.h", "#include \"../lib/io.h\"\nu8 c;"),
        ];
        let tokens = lex(&files).unwrap();
        let names: Vec<&str> = tokens
            .iter()
            .filter_map(|t| match &t.tok {
                Tok::Ident(name) if name != "u8" => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(names, ["b", "c", "a"]);
        assert_eq!(tokens[0].loc.file, 1, "tokens remember their file");
    }

    #[test]
    fn errors_say_where() {
        let err = |source: &str| {
            let e = lex(&[("a.c", source)]).unwrap_err();
            (e.pos.line, e.pos.col, e.message)
        };
        assert_eq!(err("u8 x = 12ab;"), (1, 8, "`12ab` is not a number".into()));
        assert_eq!(err("\n  x = @;"), (2, 7, "unexpected character `@`".into()));
        assert_eq!(err("x = \"open"), (1, 5, "a string needs a closing `\"`".into()));
        assert_eq!(err("/* open"), (1, 1, "a `/*` comment is never closed".into()));
        assert_eq!(err("#include \"nope.h\""), (1, 1, "cannot read `nope.h`: no such file".into()));
        assert_eq!(err("#define X 1").2, "the only `#` line there is is `#include \"file\"`");
        assert_eq!(err("x = 99999999999;").2, "`99999999999` is not a number");
        let missing = Lexer::new(&std::collections::BTreeMap::new()).run("m.c").unwrap_err();
        assert_eq!(missing.to_string(), "m.c:1:1: error: cannot read `m.c`: no such file");
    }
}
