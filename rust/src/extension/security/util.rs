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
    let offset = source.floor_char_boundary(offset.min(source.len()));
    let before = &source[..offset];
    let line = before.bytes().filter(|&b| b == b'\n').count() + 1;
    let col = match before.rfind('\n') {
        Some(i) => offset - i,
        None => offset + 1,
    };
    (line, col)
}

pub fn snippet_at(source: &str, span: Span) -> String {
    let start = source.floor_char_boundary((span.start as usize).min(source.len()));
    let end = source.ceil_char_boundary((span.end as usize).min(source.len()));
    let slice = source.get(start..end).unwrap_or("");
    let line = slice.lines().next().unwrap_or(slice);
    let mut s = line.trim().to_owned();
    if s.len() > 80 {
        s.truncate(s.floor_char_boundary(80));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_at_is_utf8_safe() {
        let source = "consolé.evil()";
        let mid_e = source.find('é').expect("é") + 1;
        assert!(!source.is_char_boundary(mid_e));
        // Mid-char start must not panic; inverted span → empty.
        let _ = snippet_at(source, Span::new(mid_e as u32, source.len() as u32));
        assert_eq!(snippet_at(source, Span::new(10, 2)), "");
    }

    #[test]
    fn offset_to_line_col_is_utf8_safe() {
        let source = "a😀b";
        let mid = source.find('😀').expect("emoji") + 1;
        assert!(!source.is_char_boundary(mid));
        let (line, _) = offset_to_line_col(source, mid);
        assert_eq!(line, 1);
    }
}
