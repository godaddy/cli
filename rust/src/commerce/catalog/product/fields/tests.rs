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
