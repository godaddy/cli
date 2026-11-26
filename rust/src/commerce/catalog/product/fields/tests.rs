use super::*;

#[test]
fn a_type_is_accepted_in_any_case_and_sent_in_upper_case() {
    for (given, sent) in [
        ("physical", "PHYSICAL"),
        ("Digital", "DIGITAL"),
        (" DIGITAL ", "DIGITAL"),
    ] {
        assert_eq!(parse_kind(given), Ok(sent.to_owned()), "{given:?}");
    }
}

#[test]
fn an_unknown_type_is_rejected_and_says_what_is_accepted() {
    for given in ["", "service", "phys", "PHYSICAL,DIGITAL"] {
        let error = parse_kind(given).expect_err("not a type");
        assert!(error.contains("physical or digital"), "{given:?}: {error}");
    }
}

#[test]
fn a_status_is_accepted_in_any_case_and_sent_in_upper_case() {
    for (given, sent) in [("draft", "DRAFT"), ("Active", "ACTIVE")] {
        assert_eq!(parse_status(given), Ok(sent.to_owned()), "{given:?}");
    }
}

/// Archiving is its own command, so asking for it here points there.
#[test]
fn archived_is_not_a_status_to_set_and_the_error_says_where_to_archive() {
    for given in ["archived", "ARCHIVED", "bogus", ""] {
        let error = parse_status(given).expect_err("not settable");
        assert!(error.contains("draft or active"), "{given:?}: {error}");
        assert!(error.contains("product archive"), "{given:?}: {error}");
    }
}

#[test]
fn a_label_is_trimmed_and_cannot_be_blank() {
    assert_eq!(parse_label("  Classic Tee  "), Ok("Classic Tee".to_owned()));
    for given in ["", "   ", "\t"] {
        assert!(parse_label(given).is_err(), "{given:?}");
    }
}

#[test]
fn a_handle_is_accepted_when_the_api_would_accept_it() {
    for given in [
        "classic-tee",
        "tee_2",
        "a",
        "0",
        "a-b_c-9",
        &"a".repeat(255),
    ] {
        assert_eq!(parse_handle(given), Ok(given.to_owned()), "{given:?}");
    }
    assert_eq!(
        parse_handle("  classic-tee  "),
        Ok("classic-tee".to_owned())
    );
}

/// Each of these the API refuses (the first three were refused live), so a person
/// is told here, in a preview as in a real run, and nothing is sent. A handle in
/// upper case is refused and not rewritten, since a different handle than the one
/// asked for is not what was asked for.
#[test]
fn a_handle_the_api_would_refuse_is_refused_here_and_says_what_is_accepted() {
    for given in [
        "Upper-Case",
        "has space",
        "Bad Name!",
        "",
        "   ",
        "tee.2",
        "tée",
        "a/b",
        &"a".repeat(256),
    ] {
        let error = parse_handle(given).expect_err("not a handle");
        assert!(
            error.contains("lowercase letters, digits"),
            "{given:?}: {error}"
        );
        assert!(error.contains("classic-tee"), "{given:?}: {error}");
    }
}
