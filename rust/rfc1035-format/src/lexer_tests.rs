use super::*;
use crate::test_support::dedent;

fn words(line: &Line) -> Vec<String> {
    line.tokens.iter().map(Token::display).collect()
}

#[test]
fn splits_lines_and_skips_blanks_and_comments() {
    let lines = lex(&dedent(
        r#"
        ; header

        www A 192.0.2.1 ; trailing

        mail A 192.0.2.2
        "#,
    ))
    .expect("lex");
    assert_eq!(lines.len(), 2);
    assert_eq!(words(&lines[0]), ["www", "A", "192.0.2.1"]);
    assert_eq!(lines[0].number, 3);
    assert_eq!(lines[1].number, 5);
}

#[test]
fn whitespace_only_lines_are_skipped() {
    let lines = lex("a A 1.2.3.4\n   \n\t\nb A 1.2.3.5\n").expect("lex");
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1].number, 4);
}

#[test]
fn parentheses_join_physical_lines() {
    let lines = lex(&dedent(
        r#"
        @ SOA ns1 admin (
          1 ; serial
          2h 15m
          2w 5m )
        www A 192.0.2.1
        "#,
    ))
    .expect("lex");
    assert_eq!(lines.len(), 2);
    assert_eq!(
        words(&lines[0]),
        ["@", "SOA", "ns1", "admin", "1", "2h", "15m", "2w", "5m"]
    );
    assert_eq!(lines[1].number, 5);
}

#[test]
fn indented_lines_are_flagged() {
    let lines = lex(&dedent(
        r#"
        www A 192.0.2.1
            A 192.0.2.2
        "#,
    ))
    .expect("lex");
    assert!(!lines[0].indented);
    assert!(lines[1].indented);

    // A tab counts as indentation too; written out because editors replace tabs.
    let lines = lex("www A 192.0.2.1\n\tAAAA ::1\n").expect("lex");
    assert!(lines[1].indented);
}

#[test]
fn quoted_strings_decode_escapes_and_keep_delimiters() {
    let lines = lex(r#"t TXT "a\"b;c (d)" "\065\\""#).expect("lex");
    let tokens = &lines[0].tokens;
    assert_eq!(tokens[2].bytes().expect("bytes"), b"a\"b;c (d)");
    assert_eq!(tokens[3].bytes().expect("bytes"), b"A\\");
}

#[test]
fn adjacent_quote_after_a_word_is_joined() {
    let lines = lex(r#"@ HTTPS 1 . alpn="h2,h3" port=443"#).expect("lex");
    let tokens = &lines[0].tokens;
    assert!(!tokens[4].joined);
    assert!(tokens[5].joined, "quoted value follows `alpn=` directly");
    assert!(!tokens[6].joined);
}

#[test]
fn words_keep_their_escapes() {
    let lines = lex(r"a\.b A 192.0.2.1").expect("lex");
    assert_eq!(lines[0].tokens[0].word(), Some(r"a\.b"));
}

#[test]
fn empty_quoted_string_is_a_token() {
    let lines = lex(r#"t TXT """#).expect("lex");
    assert_eq!(lines[0].tokens[2].bytes().expect("bytes"), b"");
}

#[test]
fn reports_unbalanced_input_with_a_position() {
    let err = lex(&dedent(
        r#"
        a A 1.2.3.4
        b TXT "open
        "#,
    ))
    .expect_err("unterminated");
    assert_eq!((err.line(), err.column()), (2, 7));
    assert_eq!(err.kind(), ErrorKind::Syntax);

    let err = lex(&dedent(
        r#"
        @ SOA (
        1 2 3
        "#,
    ))
    .expect_err("unclosed paren");
    assert_eq!(err.line(), 1);

    let err = lex("a A 1.2.3.4 )\n").expect_err("stray paren");
    assert_eq!((err.line(), err.column()), (1, 13));
}

#[test]
fn rejects_bad_escapes() {
    assert!(decode_escapes(r"\999").is_err());
    assert!(decode_escapes(r"\12").is_err());
    assert!(decode_escapes("\\").is_err());
    assert_eq!(decode_escapes(r"\032").expect("ok"), b" ");
    assert_eq!(decode_escapes(r"\é").expect("ok"), "é".as_bytes());
    assert!(lex("a\\\n").is_err());
}

#[test]
fn handles_input_without_a_trailing_newline_and_crlf() {
    let lines = lex("a A 1.2.3.4\r\nb A 1.2.3.5").expect("lex");
    assert_eq!(lines.len(), 2);
    assert_eq!(words(&lines[1]), ["b", "A", "1.2.3.5"]);
}
