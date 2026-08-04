//! The lexer: source text in, a flat list of tokens out.
//!
//! It scans byte by byte. Whitespace and `//` comments are skipped. Numbers,
//! strings, identifiers/keywords, and operators each get their own little
//! routine. Every token remembers the byte range it came from.

use crate::diagnostic::{LuxError, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    // literals
    Int(i64),
    Float(f64),
    Str(String),
    True,
    False,
    Ident(String),
    // keywords
    Let,
    Var,
    If,
    Else,
    While,
    For,
    In,
    Func,
    Return,
    Struct,
    Enum,
    Match,
    // punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Colon,
    Comma,
    Dot,
    DotDot,
    Arrow,
    FatArrow,
    // operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    PlusEq,
    MinusEq,
    EqEq,
    NotEq,
    Lt,
    Gt,
    Le,
    Ge,
    AndAnd,
    OrOr,
    Bang,
    // end of input
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

/// The language's keywords, as data rather than an inline match, so the count is a
/// single pinned source: the README and anderix.com both say "fourteen keywords",
/// the `; 14]` makes adding one a compile error, and a test asserts the number
/// alongside `BUILTINS` (#55). One entry per reserved word, paired with its token.
pub(crate) const KEYWORDS: [(&str, Tok); 14] = [
    ("let", Tok::Let),
    ("var", Tok::Var),
    ("if", Tok::If),
    ("else", Tok::Else),
    ("while", Tok::While),
    ("for", Tok::For),
    ("in", Tok::In),
    ("func", Tok::Func),
    ("return", Tok::Return),
    ("struct", Tok::Struct),
    ("enum", Tok::Enum),
    ("match", Tok::Match),
    ("true", Tok::True),
    ("false", Tok::False),
];

pub fn lex(source: &str) -> Result<Vec<Token>, LuxError> {
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < n {
        let c = bytes[i];

        // whitespace
        if c == b' ' || c == b'\t' || c == b'\r' || c == b'\n' {
            i += 1;
            continue;
        }

        // line comment
        if c == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            while i < n && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        let start = i;

        // number: int or float
        if c.is_ascii_digit() {
            while i < n && bytes[i].is_ascii_digit() {
                i += 1;
            }
            // A `.` here means a decimal point — unless it's `..`, the start of
            // a range like `0..5`, in which case this number is a plain int.
            if i < n && bytes[i] == b'.' && !(i + 1 < n && bytes[i + 1] == b'.') {
                if i + 1 < n && bytes[i + 1].is_ascii_digit() {
                    i += 1; // consume the dot
                    while i < n && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    let text = &source[start..i];
                    let val: f64 = text
                        .parse()
                        .map_err(|_| LuxError::new("invalid float literal", Span::new(start, i)))?;
                    tokens.push(Token {
                        tok: Tok::Float(val),
                        span: Span::new(start, i),
                    });
                    continue;
                } else {
                    return Err(LuxError::new(
                        "a float needs at least one digit after the decimal point",
                        Span::new(start, i + 1),
                    )
                    .with_note("write 3.0, not 3."));
                }
            }
            let text = &source[start..i];
            let val: i64 = text
                .parse()
                .map_err(|_| LuxError::new("integer literal is too large", Span::new(start, i)))?;
            tokens.push(Token {
                tok: Tok::Int(val),
                span: Span::new(start, i),
            });
            continue;
        }

        // identifier or keyword
        if c.is_ascii_alphabetic() || c == b'_' {
            while i < n && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let text = &source[start..i];
            let tok = KEYWORDS
                .iter()
                .find(|(kw, _)| *kw == text)
                .map_or_else(|| Tok::Ident(text.to_string()), |(_, t)| t.clone());
            tokens.push(Token {
                tok,
                span: Span::new(start, i),
            });
            continue;
        }

        // string literal
        if c == b'"' {
            i += 1; // opening quote
            let mut s = String::new();
            loop {
                if i >= n {
                    return Err(LuxError::new("unterminated string", Span::new(start, i))
                        .with_note("add a closing \" to the end of the string"));
                }
                let ch = bytes[i];
                if ch == b'"' {
                    i += 1; // closing quote
                    break;
                }
                if ch == b'\\' {
                    if i + 1 >= n {
                        return Err(LuxError::new("unterminated string", Span::new(start, i)));
                    }
                    let mapped = match bytes[i + 1] {
                        b'n' => '\n',
                        b't' => '\t',
                        b'"' => '"',
                        b'\\' => '\\',
                        other => {
                            return Err(LuxError::new(
                                format!("unknown escape sequence \\{}", other as char),
                                Span::new(i, i + 2),
                            )
                            .with_note("lux understands \\n, \\t, \\\" and \\\\"));
                        }
                    };
                    s.push(mapped);
                    i += 2;
                    continue;
                }
                // a newline before the closing quote almost always means a
                // missing " — stop at the opening quote, where the fix belongs,
                // instead of swallowing the rest of the file until some far-off
                // quote and blaming a line the reader never touched.
                if ch == b'\n' {
                    return Err(LuxError::new("unterminated string", Span::new(start, i))
                        .with_note("a string has to close with \" on the line it opens; add the missing \""));
                }
                // ordinary character (handle multi-byte UTF-8 safely)
                let rest = &source[i..];
                let ch_char = rest.chars().next().unwrap();
                s.push(ch_char);
                i += ch_char.len_utf8();
            }
            tokens.push(Token {
                tok: Tok::Str(s),
                span: Span::new(start, i),
            });
            continue;
        }

        // two-character operators (compared as bytes to avoid splitting UTF-8)
        let c1 = if i + 1 < n { bytes[i + 1] } else { 0 };
        let two = match (c, c1) {
            (b'=', b'=') => Some(Tok::EqEq),
            (b'!', b'=') => Some(Tok::NotEq),
            (b'<', b'=') => Some(Tok::Le),
            (b'>', b'=') => Some(Tok::Ge),
            (b'&', b'&') => Some(Tok::AndAnd),
            (b'|', b'|') => Some(Tok::OrOr),
            (b'+', b'=') => Some(Tok::PlusEq),
            (b'-', b'=') => Some(Tok::MinusEq),
            (b'-', b'>') => Some(Tok::Arrow),
            (b'=', b'>') => Some(Tok::FatArrow),
            (b'.', b'.') => Some(Tok::DotDot),
            _ => None,
        };
        if let Some(t) = two {
            tokens.push(Token {
                tok: t,
                span: Span::new(start, i + 2),
            });
            i += 2;
            continue;
        }

        // single-character tokens
        let single = match c {
            b'(' => Tok::LParen,
            b')' => Tok::RParen,
            b'{' => Tok::LBrace,
            b'}' => Tok::RBrace,
            b'[' => Tok::LBracket,
            b']' => Tok::RBracket,
            b':' => Tok::Colon,
            b',' => Tok::Comma,
            b'.' => Tok::Dot,
            b'+' => Tok::Plus,
            b'-' => Tok::Minus,
            b'*' => Tok::Star,
            b'/' => Tok::Slash,
            b'%' => Tok::Percent,
            b'=' => Tok::Eq,
            b'<' => Tok::Lt,
            b'>' => Tok::Gt,
            b'!' => Tok::Bang,
            other => {
                return Err(LuxError::new(
                    format!("unexpected character '{}'", other as char),
                    Span::new(start, start + 1),
                ));
            }
        };
        tokens.push(Token {
            tok: single,
            span: Span::new(start, start + 1),
        });
        i += 1;
    }

    tokens.push(Token {
        tok: Tok::Eof,
        span: Span::new(n, n),
    });
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::BUILTINS;

    /// The README and anderix.com/lux/ both state "fourteen keywords and fourteen
    /// built-in functions." Adding a keyword is already a compile error against the
    /// `[...; 14]` on `KEYWORDS`; this pins the built-in count the same way, so a
    /// documented number can't drift silently (#55).
    #[test]
    fn the_documented_counts_still_hold() {
        assert_eq!(KEYWORDS.len(), 14, "the README says fourteen keywords");
        assert_eq!(
            BUILTINS.len(),
            14,
            "the README says fourteen built-in functions"
        );
    }

    // The line and column a diagnostic prints come from the byte at span.start,
    // so a string missing its " must anchor there — on the line it opened —
    // not run on to a later quote and blame a line the reader never touched.
    #[test]
    fn unterminated_string_points_at_the_opening_line() {
        // The second line has a real quote; the old lexer would consume it as
        // the "close" and trip somewhere downstream. The error must stay on line 1.
        let src = "print(\"welcome to the keep)\nlet mood = \"dark\"\n";
        let err = lex(src).expect_err("missing quote should not lex");
        assert_eq!(err.message, "unterminated string");
        assert!(err.note.is_some());
        // span.start sits before any newline -> reported on the first line.
        assert!(!src[..err.span.start].contains('\n'));
    }

    // Apostrophes inside a well-formed string are ordinary text — never escaped.
    #[test]
    fn apostrophes_inside_a_string_are_fine() {
        lex("print(\"you can't see; it's dark\")\n").expect("apostrophes should lex");
    }
}
