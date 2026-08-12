//! Shared helpers for AST security scanning.

use oxc_ast::ast::{CallExpression, Expression};
use oxc_span::Span;

pub fn matches_http_url(s: &str) -> bool {
    s.contains("http://") || s.contains("https://")
}

pub fn first_string_arg<'a>(call: &CallExpression<'a>) -> Option<&'a str> {
    let expr = call.arguments.first()?.as_expression()?;
    match expr {
        Expression::StringLiteral(lit) => Some(lit.value.as_str()),
        _ => None,
    }
}

pub fn offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let before = &source[..offset];
    let line = before.bytes().filter(|&b| b == b'\n').count() + 1;
    let col = match before.rfind('\n') {
        Some(i) => offset - i,
        None => offset + 1,
    };
    (line, col)
}

pub fn snippet_at(source: &str, span: Span) -> String {
    let start = (span.start as usize).min(source.len());
    let end = (span.end as usize).min(source.len());
    let slice = &source[start..end];
    let line = slice.lines().next().unwrap_or(slice);
    let mut s = line.trim().to_owned();
    if s.len() > 80 {
        s.truncate(80);
    }
    s
}
