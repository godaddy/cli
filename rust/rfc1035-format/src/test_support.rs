//! Helpers shared by the unit tests.

/// Lay out zone text in a test the way it would look in a file.
///
/// Write the text as a raw string that starts on the line after the opening
/// quote and ends on the line of the closing one, indented to match the code:
///
/// ```ignore
/// let zone = dedent(r#"
///     www A 192.0.2.1
///         A 192.0.2.2
/// "#);
/// ```
///
/// The first newline and the closing line's indentation are dropped, and the
/// indentation shared by every non-blank line is removed. Indentation beyond
/// that is kept, because in a zone file it means something (a line that starts
/// with whitespace reuses the previous owner name). The result always ends in a
/// newline, and line numbers start at 1 on the first line of text.
pub(crate) fn dedent(text: &str) -> String {
    let text = text.strip_prefix('\n').unwrap_or(text);
    let text = text.trim_end_matches([' ', '\t']);
    let indent = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    let mut out = String::with_capacity(text.len() + 1);
    for line in text.lines() {
        // A blank line may be shorter than the indent; it becomes empty.
        out.push_str(line.get(indent..).unwrap_or(""));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::dedent;

    #[test]
    fn removes_the_shared_indent_and_the_framing_lines() {
        let text = r#"
            www A 192.0.2.1
            mail A 192.0.2.2
        "#;
        assert_eq!(dedent(text), "www A 192.0.2.1\nmail A 192.0.2.2\n");
    }

    #[test]
    fn keeps_indentation_beyond_the_shared_amount() {
        let text = r#"
            www A 192.0.2.1
                A 192.0.2.2
        "#;
        assert_eq!(dedent(text), "www A 192.0.2.1\n    A 192.0.2.2\n");
    }

    #[test]
    fn blank_lines_stay_blank_and_do_not_set_the_indent() {
        let text = "\n    a\n\n  \n    b\n";
        assert_eq!(dedent(text), "a\n\n\nb\n");
    }

    #[test]
    fn leaves_unindented_single_lines_alone_apart_from_the_final_newline() {
        assert_eq!(dedent("a A 1.2.3.4\n"), "a A 1.2.3.4\n");
        assert_eq!(dedent("a A 1.2.3.4"), "a A 1.2.3.4\n");
        assert_eq!(dedent(""), "");
    }
}
