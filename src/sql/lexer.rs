//! SQL lexer with byte offsets for diagnostics.

use std::io;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Word(String),
    Integer(i64),
    String(String),
    Comma,
    LeftParen,
    RightParen,
    Star,
    Semicolon,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    End,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub pos: usize,
}

pub fn lex(source: &str) -> io::Result<Vec<Token>> {
    let bytes = source.as_bytes();
    let mut offset = 0;
    let mut tokens = Vec::new();
    while offset < bytes.len() {
        if bytes[offset].is_ascii_whitespace() {
            offset += 1;
            continue;
        }
        let pos = offset;
        let kind = match bytes[offset] {
            b',' => {
                offset += 1;
                TokenKind::Comma
            }
            b'(' => {
                offset += 1;
                TokenKind::LeftParen
            }
            b')' => {
                offset += 1;
                TokenKind::RightParen
            }
            b'*' => {
                offset += 1;
                TokenKind::Star
            }
            b';' => {
                offset += 1;
                TokenKind::Semicolon
            }
            b'=' => {
                offset += 1;
                TokenKind::Eq
            }
            b'!' if bytes.get(offset + 1) == Some(&b'=') => {
                offset += 2;
                TokenKind::Ne
            }
            b'<' if bytes.get(offset + 1) == Some(&b'=') => {
                offset += 2;
                TokenKind::Le
            }
            b'>' if bytes.get(offset + 1) == Some(&b'=') => {
                offset += 2;
                TokenKind::Ge
            }
            b'<' => {
                offset += 1;
                TokenKind::Lt
            }
            b'>' => {
                offset += 1;
                TokenKind::Gt
            }
            b'\'' => {
                offset += 1;
                let mut value = String::new();
                loop {
                    let Some(&byte) = bytes.get(offset) else {
                        return Err(error(pos, "unterminated string literal"));
                    };
                    if byte == b'\'' {
                        if bytes.get(offset + 1) == Some(&b'\'') {
                            value.push('\'');
                            offset += 2;
                        } else {
                            offset += 1;
                            break;
                        }
                    } else {
                        let tail = &source[offset..];
                        let character = tail
                            .chars()
                            .next()
                            .ok_or_else(|| error(pos, "unterminated string literal"))?;
                        value.push(character);
                        offset += character.len_utf8();
                    }
                }
                TokenKind::String(value)
            }
            b'-' | b'0'..=b'9' => {
                let start = offset;
                if bytes[offset] == b'-' {
                    offset += 1;
                    if !bytes.get(offset).map(|byte| byte.is_ascii_digit()).unwrap_or(false) {
                        return Err(error(pos, "expected digits after '-'"));
                    }
                }
                while bytes.get(offset).map(|byte| byte.is_ascii_digit()).unwrap_or(false) {
                    offset += 1;
                }
                let value = source[start..offset]
                    .parse::<i64>()
                    .map_err(|_| error(pos, "integer literal out of range"))?;
                TokenKind::Integer(value)
            }
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let start = offset;
                offset += 1;
                while bytes
                    .get(offset)
                    .map(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                    .unwrap_or(false)
                {
                    offset += 1;
                }
                TokenKind::Word(source[start..offset].to_ascii_uppercase())
            }
            other => {
                return Err(error(
                    pos,
                    &format!("unexpected character '{}'", other as char),
                ))
            }
        };
        tokens.push(Token { kind, pos });
    }
    tokens.push(Token {
        kind: TokenKind::End,
        pos: source.len(),
    });
    Ok(tokens)
}

pub fn error(pos: usize, message: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("sql at byte {pos}: {message}"),
    )
}
