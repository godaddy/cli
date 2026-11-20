//! Domain names in presentation format.

use std::fmt;
use std::str::FromStr;

use crate::error::{ErrorKind, ParseError};
use crate::lexer::decode_escapes;

const MAX_LABEL_LEN: usize = 63;
/// Longest name in wire format: every label plus its length byte, plus the root.
const MAX_NAME_LEN: usize = 255;

/// An absolute domain name, kept as its labels.
///
/// Labels are raw bytes, so a name read from `a\.b.example.com.` has the labels
/// `a.b`, `example` and `com`. Comparison is exact; DNS treats ASCII case as
/// insignificant, so lower-case both sides first when that matters.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Name {
    labels: Vec<Vec<u8>>,
}

impl Name {
    /// The root name, written `.`.
    pub fn root() -> Self {
        Self::default()
    }

    /// Whether this is the root name.
    pub fn is_root(&self) -> bool {
        self.labels.is_empty()
    }

    /// The labels, most specific first (`www`, `example`, `com`).
    pub fn labels(&self) -> &[Vec<u8>] {
        &self.labels
    }

    /// Build a name from labels, most specific first.
    ///
    /// # Errors
    ///
    /// A label is empty or longer than 63 bytes, or the whole name is longer than
    /// 255 bytes in wire format.
    pub fn from_labels(labels: Vec<Vec<u8>>) -> Result<Self, ParseError> {
        let mut wire_len = 1;
        for label in &labels {
            if label.is_empty() {
                return Err(ParseError::new(
                    ErrorKind::InvalidValue,
                    0,
                    0,
                    "a name has an empty label",
                ));
            }
            if label.len() > MAX_LABEL_LEN {
                return Err(ParseError::new(
                    ErrorKind::InvalidValue,
                    0,
                    0,
                    format!(
                        "label `{}` is {} bytes; a label is at most {MAX_LABEL_LEN}",
                        label_text(label),
                        label.len()
                    ),
                ));
            }
            wire_len += label.len() + 1;
        }
        if wire_len > MAX_NAME_LEN {
            return Err(ParseError::new(
                ErrorKind::InvalidValue,
                0,
                0,
                format!("a name is at most {MAX_NAME_LEN} bytes; this one is {wire_len}"),
            ));
        }
        Ok(Self { labels })
    }

    /// Read a name. `@` is `origin`; a name that does not end in `.` has `origin`
    /// appended.
    ///
    /// # Errors
    ///
    /// The text is not a valid name, or it is relative (or `@`) and `origin` is
    /// `None`.
    pub fn parse(text: &str, origin: Option<&Name>) -> Result<Self, ParseError> {
        Self::parse_at(text, origin, 0, 0)
    }

    pub(crate) fn parse_at(
        text: &str,
        origin: Option<&Name>,
        line: usize,
        column: usize,
    ) -> Result<Self, ParseError> {
        let fail =
            |message: String| ParseError::new(ErrorKind::InvalidValue, line, column, message);
        if text == "@" {
            return origin
                .cloned()
                .ok_or_else(|| fail("`@` needs an origin; add a `$ORIGIN` line".to_owned()));
        }
        if text == "." {
            return Ok(Self::root());
        }
        let (raw_labels, absolute) = split_labels(text).map_err(fail)?;
        let mut labels = Vec::with_capacity(raw_labels.len());
        for raw in raw_labels {
            labels.push(
                decode_escapes(raw)
                    .map_err(|m| ParseError::new(ErrorKind::InvalidValue, line, column, m))?,
            );
        }
        if !absolute {
            let origin = origin.ok_or_else(|| {
                ParseError::new(
                    ErrorKind::InvalidValue,
                    line,
                    column,
                    format!("`{text}` is relative and no origin is set; add a `$ORIGIN` line"),
                )
            })?;
            labels.extend(origin.labels.iter().cloned());
        }
        Self::from_labels(labels).map_err(|e| ParseError::new(e.kind(), line, column, e.message()))
    }

    /// Whether this name is `other` or lies below it (ASCII case ignored).
    pub fn is_subdomain_of(&self, other: &Name) -> bool {
        let Some(skip) = self.labels.len().checked_sub(other.labels.len()) else {
            return false;
        };
        self.labels[skip..]
            .iter()
            .zip(&other.labels)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
    }

    /// This name relative to `origin`: `@` for the origin itself, `www` for
    /// `www.example.com.` under `example.com.`. `None` when the name is not at or
    /// below `origin`.
    pub fn relative_to(&self, origin: &Name) -> Option<String> {
        if !self.is_subdomain_of(origin) {
            return None;
        }
        let keep = self.labels.len() - origin.labels.len();
        if keep == 0 {
            return Some("@".to_owned());
        }
        Some(
            self.labels[..keep]
                .iter()
                .map(|label| label_text(label))
                .collect::<Vec<_>>()
                .join("."),
        )
    }
}

/// Split `text` at unescaped dots. The flag is whether it ended in one, which
/// makes the name absolute.
fn split_labels(text: &str) -> Result<(Vec<&str>, bool), String> {
    if text.is_empty() {
        return Err("a name is empty".to_owned());
    }
    let bytes = text.as_bytes();
    let mut labels = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 1;
                if bytes.get(i).is_some_and(u8::is_ascii_digit) {
                    i += 3;
                } else {
                    i += text
                        .get(i..)
                        .and_then(|rest| rest.chars().next())
                        .map_or(1, char::len_utf8);
                }
            }
            b'.' => {
                if i == start {
                    return Err(format!("`{text}` has an empty label"));
                }
                labels.push(&text[start..i]);
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    let absolute = start == bytes.len();
    if !absolute {
        labels.push(&text[start..]);
    }
    Ok((labels, absolute))
}

/// One label as it is written: characters that would end or change a token are
/// escaped, and anything unprintable is `\DDD`.
fn label_text(label: &[u8]) -> String {
    let mut out = String::with_capacity(label.len());
    for &byte in label {
        match byte {
            b'.' | b'\\' | b'"' | b';' | b'(' | b')' | b'@' | b'$' => {
                out.push('\\');
                out.push(char::from(byte));
            }
            0x21..=0x7e => out.push(char::from(byte)),
            _ => out.push_str(&format!("\\{byte:03}")),
        }
    }
    out
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_root() {
            return f.write_str(".");
        }
        for label in &self.labels {
            write!(f, "{}.", label_text(label))?;
        }
        Ok(())
    }
}

impl FromStr for Name {
    type Err = ParseError;

    /// Read an absolute name such as `example.com.`.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text, None)
    }
}

#[cfg(test)]
#[path = "name_tests.rs"]
mod tests;
