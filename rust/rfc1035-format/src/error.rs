//! Errors reported while parsing a zone file.

use std::fmt;

/// What kind of problem a [`ParseError`] reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The text is not shaped like a master file (unbalanced quote or
    /// parenthesis, missing field, extra field).
    Syntax,
    /// The text has the right shape but a value is not acceptable (a number out
    /// of range, a malformed address or name).
    InvalidValue,
    /// The record type is neither built in nor registered with
    /// [`ParseOptions::with_custom_type`](crate::ParseOptions::with_custom_type).
    UnknownType,
    /// The input uses a feature this crate deliberately does not implement
    /// (`$INCLUDE`, `$GENERATE`, classes other than `IN`).
    Unsupported,
    /// The input exceeds a limit in [`ParseOptions`](crate::ParseOptions).
    Limit,
}

/// A problem found at a specific place in the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    kind: ErrorKind,
    line: usize,
    column: usize,
    message: String,
}

impl ParseError {
    pub(crate) fn new(
        kind: ErrorKind,
        line: usize,
        column: usize,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            line,
            column,
            message: message.into(),
        }
    }

    /// The category of the problem.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The 1-based line number the problem was found on (0 when it concerns the
    /// input as a whole).
    pub fn line(&self) -> usize {
        self.line
    }

    /// The 1-based byte column within the line (0 when unknown).
    pub fn column(&self) -> usize {
        self.column
    }

    /// A description of the problem, without the position.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.column) {
            (0, _) => f.write_str(&self.message),
            (line, 0) => write!(f, "line {line}: {}", self.message),
            (line, column) => write!(f, "line {line}, column {column}: {}", self.message),
        }
    }
}

impl std::error::Error for ParseError {}
