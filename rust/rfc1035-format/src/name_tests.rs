use super::*;

fn origin() -> Name {
    "example.com.".parse().expect("origin")
}

#[test]
fn absolute_relative_and_apex_names() {
    let origin = origin();
    assert_eq!(
        Name::parse("www.example.com.", Some(&origin))
            .expect("abs")
            .to_string(),
        "www.example.com."
    );
    assert_eq!(
        Name::parse("www", Some(&origin)).expect("rel").to_string(),
        "www.example.com."
    );
    assert_eq!(Name::parse("@", Some(&origin)).expect("apex"), origin);
    assert_eq!(Name::parse(".", None).expect("root"), Name::root());
}

#[test]
fn relative_names_and_apex_need_an_origin() {
    assert!(Name::parse("www", None).is_err());
    assert!(Name::parse("@", None).is_err());
    assert!(Name::parse("www.example.com.", None).is_ok());
}

#[test]
fn escaped_dots_stay_inside_a_label() {
    let name: Name = r"a\.b.example.com.".parse().expect("name");
    assert_eq!(name.labels().len(), 3);
    assert_eq!(name.labels()[0], b"a.b");
    assert_eq!(name.to_string(), r"a\.b.example.com.");
}

#[test]
fn numeric_escapes_round_trip() {
    let name: Name = r"a\032b.example.com.".parse().expect("name");
    assert_eq!(name.labels()[0], b"a b");
    assert_eq!(name.to_string(), r"a\032b.example.com.");
}

#[test]
fn rejects_empty_and_oversized_labels_and_names() {
    assert!(Name::parse("a..b.", None).is_err());
    assert!(Name::parse(".a.", None).is_err());
    assert!(Name::parse("", None).is_err());
    let long_label = format!("{}.com.", "a".repeat(64));
    assert!(Name::parse(&long_label, None).is_err());
    let ok_label = format!("{}.com.", "a".repeat(63));
    assert!(Name::parse(&ok_label, None).is_ok());
    let long_name = format!("{}com.", "abcdefghi.".repeat(26));
    assert!(Name::parse(&long_name, None).is_err());
}

#[test]
fn relative_to_origin() {
    let origin = origin();
    let www: Name = "www.example.com.".parse().expect("name");
    assert_eq!(www.relative_to(&origin).as_deref(), Some("www"));
    assert_eq!(origin.relative_to(&origin).as_deref(), Some("@"));
    let deep: Name = "_443._tcp.www.example.com.".parse().expect("name");
    assert_eq!(deep.relative_to(&origin).as_deref(), Some("_443._tcp.www"));
    let other: Name = "www.example.org.".parse().expect("name");
    assert_eq!(other.relative_to(&origin), None);
    let shorter: Name = "com.".parse().expect("name");
    assert_eq!(shorter.relative_to(&origin), None);
}

#[test]
fn subdomain_check_ignores_case_and_label_boundaries() {
    let origin = origin();
    let upper: Name = "WWW.EXAMPLE.COM.".parse().expect("name");
    assert!(upper.is_subdomain_of(&origin));
    let lookalike: Name = "notexample.com.".parse().expect("name");
    assert!(!lookalike.is_subdomain_of(&origin));
}

#[test]
fn special_characters_in_labels_are_escaped_on_output() {
    let name = Name::from_labels(vec![b"a@b".to_vec(), b"com".to_vec()]).expect("name");
    assert_eq!(name.to_string(), r"a\@b.com.");
    let reread: Name = name.to_string().parse().expect("reparse");
    assert_eq!(reread, name);
}
