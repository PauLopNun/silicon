//! Splitting one line of assembly into tokens.

/// An error at a 1-based column of the line being read.
pub(crate) type LineError = (u32, String);

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Tok {
    /// A name: mnemonic, register, symbol, `.directive` or `.local`.
    Ident(String),
    Num(i64),
    Str(Vec<u8>),
    /// One of `, [ ] ( ) + - * / % & | ^ ~ : = $`.
    Punct(char),
    Shl,
    Shr,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Token {
    pub tok: Tok,
    /// 1-based column where the token starts.
    pub col: u32,
}

const PUNCTUATION: &str = ",[]()+-*/%&|^~:=$";

/// Tokenize a line. A `;` starts a comment that runs to the end of the line.
pub(crate) fn lex(line: &str) -> Result<Vec<Token>, LineError> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let col = i as u32 + 1;
        let is_name = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.';
        let tok = if c.is_whitespace() {
            i += 1;
            continue;
        } else if c == ';' {
            break;
        } else if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            Tok::Num(number(&text).ok_or_else(|| (col, format!("`{text}` is not a number")))?)
        } else if is_name(c) {
            let start = i;
            while i < chars.len() && is_name(chars[i]) {
                i += 1;
            }
            Tok::Ident(chars[start..i].iter().collect())
        } else if c == '\'' {
            i += 1;
            let value = character(&chars, &mut i, col)?;
            if chars.get(i) != Some(&'\'') {
                return Err((col, "character literal needs a closing `'`".into()));
            }
            i += 1;
            Tok::Num(value as i64)
        } else if c == '"' {
            i += 1;
            let mut bytes = Vec::new();
            loop {
                match chars.get(i) {
                    None => return Err((col, "string needs a closing `\"`".into())),
                    Some('"') => break,
                    Some(_) => bytes.push(character(&chars, &mut i, col)?),
                }
            }
            i += 1;
            Tok::Str(bytes)
        } else if c == '<' || c == '>' {
            if chars.get(i + 1) != Some(&c) {
                return Err((col, format!("unexpected `{c}` (shifts are `<<` and `>>`)")));
            }
            i += 2;
            if c == '<' {
                Tok::Shl
            } else {
                Tok::Shr
            }
        } else if PUNCTUATION.contains(c) {
            i += 1;
            Tok::Punct(c)
        } else {
            return Err((col, format!("unexpected character `{c}`")));
        };
        out.push(Token { tok, col });
    }
    Ok(out)
}

/// `42`, `0x2a`, `0b101010`, with `_` allowed between digits.
fn number(text: &str) -> Option<i64> {
    let digits: String = text.chars().filter(|&c| c != '_').collect();
    let lower = digits.to_ascii_lowercase();
    if let Some(hex) = lower.strip_prefix("0x") {
        i64::from_str_radix(hex, 16).ok()
    } else if let Some(bin) = lower.strip_prefix("0b") {
        i64::from_str_radix(bin, 2).ok()
    } else {
        lower.parse().ok()
    }
}

/// One character of a string or character literal, handling `\` escapes.
fn character(chars: &[char], i: &mut usize, col: u32) -> Result<u8, LineError> {
    let c = *chars
        .get(*i)
        .ok_or_else(|| (col, "unfinished literal".to_string()))?;
    *i += 1;
    if c != '\\' {
        return u8::try_from(c as u32)
            .ok()
            .filter(u8::is_ascii)
            .ok_or_else(|| (col, format!("`{c}` is not an ASCII character")));
    }
    let escape = chars.get(*i).copied();
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
            let hex: String = chars.iter().skip(*i).take(2).collect();
            *i += 2;
            u8::from_str_radix(&hex, 16)
                .map_err(|_| (col, "`\\x` needs two hexadecimal digits".to_string()))?
        }
        other => {
            let shown = other.map(String::from).unwrap_or_default();
            return Err((col, format!("unknown escape `\\{shown}`")));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(line: &str) -> Vec<Tok> {
        lex(line).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn instructions_and_comments() {
        use Tok::*;
        assert_eq!(
            toks("loop:  ld a, [x+]  ; next byte"),
            [
                Ident("loop".into()),
                Punct(':'),
                Ident("ld".into()),
                Ident("a".into()),
                Punct(','),
                Punct('['),
                Ident("x".into()),
                Punct('+'),
                Punct(']'),
            ]
        );
        assert_eq!(toks("   ; nothing here"), []);
        assert_eq!(lex("  ld a, 1").unwrap()[0].col, 3);
    }

    #[test]
    fn numbers_characters_and_strings() {
        use Tok::*;
        assert_eq!(
            toks("10 0x1F 0b1010_0101 'A' '\\n' 1_000"),
            [Num(10), Num(31), Num(0xA5), Num(65), Num(10), Num(1000)]
        );
        assert_eq!(
            toks(r#""hi\n" "a;b""#),
            [Str(b"hi\n".to_vec()), Str(b"a;b".to_vec())]
        );
        assert_eq!(toks(r#""\x41\0\"""#), [Str(vec![0x41, 0, b'"'])]);
        assert_eq!(toks("1 << 2 >> 3"), [Num(1), Shl, Num(2), Shr, Num(3)]);
        assert_eq!(
            toks(".byte .loop a.b"),
            [
                Ident(".byte".into()),
                Ident(".loop".into()),
                Ident("a.b".into())
            ]
        );
    }

    #[test]
    fn errors_point_at_the_column() {
        assert_eq!(
            lex("ld a, 0xZZ").unwrap_err(),
            (7, "`0xZZ` is not a number".into())
        );
        assert_eq!(lex("ld a, #1").unwrap_err().0, 7);
        assert_eq!(lex(".byte \"open").unwrap_err().0, 7);
        assert_eq!(lex("ld a, 'ab'").unwrap_err().0, 7);
        assert_eq!(lex("ld a, 1 < 2").unwrap_err().0, 9);
        assert!(lex("ld a, 'é'").unwrap_err().1.contains("ASCII"));
        assert!(lex(r#".byte "\q""#)
            .unwrap_err()
            .1
            .contains("unknown escape"));
    }
}
