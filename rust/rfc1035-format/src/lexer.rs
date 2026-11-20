//! Splits master file text into logical lines of tokens.
//!
//! A logical line is what RFC 1035 §5.1 calls an entry: physical lines are
//! joined while a `(` is open, `;` starts a comment, and `"` starts a quoted
//! string. Everything else is a word, kept exactly as written (escapes
//! included) so that names keep their `\.` and `\DDD` sequences.

use crate::error::{ErrorKind, ParseError};

/// What a token holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TokenKind {
    /// Unquoted text, with escapes still in place.
    Word(String),
    /// A quoted string with its escapes already decoded.
    Quoted(Vec<u8>),
}

/// One word or quoted string, with where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Token {
    pub(crate) kind: TokenKind,
    pub(crate) line: usize,
    pub(crate) column: usize,
    /// No whitespace separates this token from the one before it, as in
    /// `alpn="h2,h3"` (a word followed by a quoted string).
    pub(crate) joined: bool,
}

impl Token {
    /// The raw text of a word, or `None` for a quoted string.
    pub(crate) fn word(&self) -> Option<&str> {
        match &self.kind {
            TokenKind::Word(text) => Some(text),
            TokenKind::Quoted(_) => None,
        }
    }

    pub(crate) fn error(&self, kind: ErrorKind, message: impl Into<String>) -> ParseError {
        ParseError::new(kind, self.line, self.column, message)
    }

    /// The token as a character-string: a quoted string's bytes, or a word's
    /// bytes with escapes decoded.
    pub(crate) fn bytes(&self) -> Result<Vec<u8>, ParseError> {
        match &self.kind {
            TokenKind::Quoted(bytes) => Ok(bytes.clone()),
            TokenKind::Word(raw) => {
                decode_escapes(raw).map_err(|message| self.error(ErrorKind::InvalidValue, message))
            }
        }
    }

    /// The token as it would be written back: a word as is, a quoted string
    /// re-quoted.
    pub(crate) fn display(&self) -> String {
        match &self.kind {
            TokenKind::Word(raw) => raw.clone(),
            TokenKind::Quoted(bytes) => crate::writer::quote(bytes),
        }
    }
}

/// One entry of the file: the tokens of a logical line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Line {
    /// 1-based number of the physical line the entry starts on.
    pub(crate) number: usize,
    /// The entry starts with whitespace, so its owner name is omitted.
    pub(crate) indented: bool,
    pub(crate) tokens: Vec<Token>,
}

fn is_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b' ' | b'\t' | b'\r' | b'\n' | b';' | b'(' | b')' | b'"'
    )
}

/// Decode the escapes in `raw`: `\DDD` (three decimal digits, at most 255) is
/// one byte, and `\X` for any other `X` is `X` itself.
pub(crate) fn decode_escapes(raw: &str) -> Result<Vec<u8>, String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        i += 1;
        let Some(&next) = bytes.get(i) else {
            return Err("a backslash at the end of the text escapes nothing".to_owned());
        };
        if next.is_ascii_digit() {
            let digits = bytes
                .get(i..i + 3)
                .filter(|d| d.iter().all(u8::is_ascii_digit));
            let Some(digits) = digits else {
                return Err("`\\` followed by a digit needs three digits, as in `\\032`".to_owned());
            };
            let value = digits
                .iter()
                .fold(0u16, |acc, d| acc * 10 + u16::from(d - b'0'));
            let byte = u8::try_from(value).map_err(|_| {
                format!(
                    "escape `\\{}` is above 255",
                    String::from_utf8_lossy(digits)
                )
            })?;
            out.push(byte);
            i += 3;
        } else {
            // Copy the escaped character whole, so a multi-byte one stays intact.
            let ch_len = raw[i..].chars().next().map_or(1, char::len_utf8);
            out.extend_from_slice(&bytes[i..i + ch_len]);
            i += ch_len;
        }
    }
    Ok(out)
}

/// Split `input` into logical lines. Blank lines and comments yield nothing.
pub(crate) fn lex(input: &str) -> Result<Vec<Line>, ParseError> {
    let bytes = input.as_bytes();
    let mut lines = Vec::new();
    let mut current: Option<Line> = None;
    let mut i = 0;
    let mut line = 1;
    let mut line_start = 0;
    let mut depth = 0usize;
    let mut paren_line = 0;
    let mut prev_end: Option<usize> = None;

    while i < bytes.len() {
        let byte = bytes[i];
        match byte {
            b'\n' => {
                if depth == 0 {
                    lines.extend(current.take());
                }
                line += 1;
                i += 1;
                line_start = i;
                prev_end = None;
            }
            b' ' | b'\t' | b'\r' => i += 1,
            b';' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'(' => {
                if depth == 0 {
                    paren_line = line;
                }
                depth += 1;
                i += 1;
            }
            b')' => {
                if depth == 0 {
                    return Err(ParseError::new(
                        ErrorKind::Syntax,
                        line,
                        i - line_start + 1,
                        "`)` without a matching `(`",
                    ));
                }
                depth -= 1;
                i += 1;
            }
            _ => {
                let start = i;
                let column = start - line_start + 1;
                let joined = prev_end == Some(start);
                let kind = if byte == b'"' {
                    let (decoded, end) = lex_quoted(input, start, line, column)?;
                    i = end;
                    TokenKind::Quoted(decoded)
                } else {
                    i = lex_word(input, start, line, column)?;
                    TokenKind::Word(input[start..i].to_owned())
                };
                prev_end = Some(i);
                let entry = current.get_or_insert_with(|| Line {
                    number: line,
                    indented: matches!(bytes.get(line_start), Some(b' ' | b'\t')),
                    tokens: Vec::new(),
                });
                entry.tokens.push(Token {
                    kind,
                    line,
                    column,
                    joined,
                });
            }
        }
    }
    if depth > 0 {
        return Err(ParseError::new(
            ErrorKind::Syntax,
            paren_line,
            0,
            "`(` is never closed",
        ));
    }
    lines.extend(current);
    Ok(lines)
}

/// Scan the quoted string that starts at `start` (the opening quote). Returns
/// the decoded bytes and the offset just past the closing quote.
fn lex_quoted(
    input: &str,
    start: usize,
    line: usize,
    column: usize,
) -> Result<(Vec<u8>, usize), ParseError> {
    let bytes = input.as_bytes();
    let mut i = start + 1;
    let raw_start = i;
    let mut decoded = Vec::new();
    loop {
        match bytes.get(i) {
            None | Some(b'\n') => {
                return Err(ParseError::new(
                    ErrorKind::Syntax,
                    line,
                    column,
                    "quoted string is not closed before the end of the line",
                ));
            }
            Some(b'"') => {
                push_decoded(&mut decoded, &input[raw_start..i], line, column)?;
                return Ok((decoded, i + 1));
            }
            Some(b'\\') => {
                // Skip the escaped character so `\"` does not end the string.
                i += 1;
                match input[i..].chars().next() {
                    Some('\n') | None => {
                        return Err(ParseError::new(
                            ErrorKind::Syntax,
                            line,
                            column,
                            "quoted string is not closed before the end of the line",
                        ));
                    }
                    Some(ch) => i += ch.len_utf8(),
                }
            }
            Some(_) => i += 1,
        }
    }
}

fn push_decoded(
    out: &mut Vec<u8>,
    raw: &str,
    line: usize,
    column: usize,
) -> Result<(), ParseError> {
    let bytes = decode_escapes(raw)
        .map_err(|message| ParseError::new(ErrorKind::InvalidValue, line, column, message))?;
    out.extend_from_slice(&bytes);
    Ok(())
}

/// Scan the word that starts at `start`. Returns the offset just past it.
fn lex_word(input: &str, start: usize, line: usize, column: usize) -> Result<usize, ParseError> {
    let bytes = input.as_bytes();
    let mut i = start;
    while let Some(&byte) = bytes.get(i) {
        if is_delimiter(byte) {
            break;
        }
        if byte == b'\\' {
            i += 1;
            match input[i..].chars().next() {
                Some('\n') | None => {
                    return Err(ParseError::new(
                        ErrorKind::Syntax,
                        line,
                        column,
                        "a backslash at the end of a line escapes nothing",
                    ));
                }
                Some(ch) => i += ch.len_utf8(),
            }
        } else {
            i += 1;
        }
    }
    Ok(i)
}

#[cfg(test)]
#[path = "lexer_tests.rs"]
mod tests;
