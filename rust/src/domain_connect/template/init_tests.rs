use clap::Parser;
use serde_json::Value;

use super::*;

#[derive(Parser)]
struct Harness {
    #[command(flatten)]
    args: InitArgs,
}

fn parse(extra: &[&str]) -> InitArgs {
    let base = [
        "init",
        "--provider-id",
        "acme.com",
        "--provider-name",
        "Acme Corp",
        "--service-id",
        "email",
        "--service-name",
        "Acme Email",
    ];
    Harness::try_parse_from(base.iter().chain(extra))
        .expect("arguments parse")
        .args
}

fn fix_of(err: &cli_engine::CliCoreError) -> String {
    cli_engine::build_error_envelope(err, SYSTEM)
        .fix
        .unwrap_or_default()
}

fn written(dir: &Path, name: &str) -> Value {
    let text = std::fs::read_to_string(dir.join(name)).expect("template file exists");
    serde_json::from_str(&text).expect("template file is JSON")
}

#[test]
fn flags_build_a_template_and_write_it_to_the_named_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let args = parse(&[
        "--description",
        "Mail setup",
        "--add-mx",
        "value=mail.acme.com,priority=10",
        "--add-a",
        "host=www,value=192.0.2.1",
        "--add-txt",
        "data=v=spf1 include:_spf.acme.com ~all",
        "--group-id",
        "mail",
        "--essential",
        "onapply",
    ]);

    let template = from_flags(&args).expect("template");
    let result = write_template(&template, dir.path(), false, false).expect("written");

    assert_eq!(result.data["action"], "created");
    assert_eq!(result.data["recordCount"], 3);
    assert!(!result.metadata.dry_run);
    let file = written(dir.path(), "acme.com.email.json");
    assert_eq!(file["providerId"], "acme.com");
    assert_eq!(file["description"], "Mail setup");
    assert_eq!(file["version"], 1);
    // Records keep the order of the record-type flags, A before MX before TXT.
    assert_eq!(file["records"][0]["type"], "A");
    assert_eq!(file["records"][0]["pointsTo"], "192.0.2.1");
    assert_eq!(file["records"][1]["type"], "MX");
    assert_eq!(file["records"][1]["groupId"], "mail");
    assert_eq!(file["records"][1]["essential"], "OnApply");
    assert_eq!(
        file["records"][2]["data"],
        "v=spf1 include:_spf.acme.com ~all"
    );
    assert_eq!(result.data["template"], file);
}

#[test]
fn the_next_action_validates_the_written_file_and_never_submits() {
    let dir = tempfile::tempdir().expect("tempdir");
    let template = from_flags(&parse(&[])).expect("template");

    let result = write_template(&template, dir.path(), false, false).expect("written");

    let actions = &result.metadata.next_actions;
    assert_eq!(actions.len(), 1);
    assert_eq!(
        actions[0].command,
        "gddy domain-connect template validate <file>"
    );
    assert_eq!(
        actions[0].params["file"].value.as_deref(),
        result.data["file"].as_str()
    );
}

#[test]
fn an_existing_file_is_kept_unless_force_is_given() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("acme.com.email.json");
    std::fs::write(&path, "mine").expect("seed file");
    let template = from_flags(&parse(&[])).expect("template");

    let err = write_template(&template, dir.path(), false, false).expect_err("refused");
    assert!(err.to_string().contains("already exists"), "{err}");
    assert!(fix_of(&err).contains("--force"), "{}", fix_of(&err));
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "mine");

    let result = write_template(&template, dir.path(), true, false).expect("overwritten");
    assert_eq!(result.data["action"], "overwritten");
    assert_eq!(
        written(dir.path(), "acme.com.email.json")["serviceId"],
        "email"
    );
}

#[test]
fn a_dry_run_reports_what_it_would_do_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let template = from_flags(&parse(&[])).expect("template");

    let result = write_template(&template, dir.path(), false, true).expect("preview");

    assert!(result.metadata.dry_run);
    assert_eq!(result.data["action"], "would create");
    assert_eq!(result.data["providerId"], "acme.com");
    assert!(!dir.path().join("acme.com.email.json").exists());
}

#[test]
fn a_dry_run_fails_where_the_real_run_would() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("acme.com.email.json"), "mine").expect("seed file");
    let template = from_flags(&parse(&[])).expect("template");

    let err = write_template(&template, dir.path(), false, true).expect_err("refused");
    assert!(err.to_string().contains("already exists"), "{err}");

    let preview = write_template(&template, dir.path(), true, true).expect("preview");
    assert_eq!(preview.data["action"], "would overwrite");
}

#[test]
fn a_provider_id_cannot_point_outside_the_current_directory() {
    for bad in ["../x.com", "a/b.com", "acme", "ACME.com", "acme.com/"] {
        let mut args = parse(&[]);
        args.provider_id = bad.to_owned();
        let err = from_flags(&args).expect_err(bad);
        assert!(err.to_string().contains("domain name"), "{bad}: {err}");
    }
}

#[test]
fn write_template_rechecks_ids_from_any_source() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut template = from_flags(&parse(&[])).expect("template");
    template.provider_id = "../x.com".to_owned();

    write_template(&template, dir.path(), true, false).expect_err("escape refused");
    assert_eq!(
        std::fs::read_dir(dir.path()).expect("list").count(),
        0,
        "nothing may be written"
    );
}

#[test]
fn a_service_id_is_limited_to_lowercase_letters_digits_hyphens_and_underscores() {
    for good in ["email", "managed-wordpress", "hosting_and_email", "v2"] {
        assert!(
            check_service_id(good).is_ok(),
            "{good:?} should be accepted"
        );
    }
    for bad in ["", "sérvice", "Email", "a.b", "a/b", "a b"] {
        assert!(check_service_id(bad).is_err(), "{bad:?} should be refused");
    }
}

#[test]
fn a_logo_url_must_be_a_web_link_to_an_image() {
    for good in ["https://acme.com/logo.svg", "http://acme.com/a/logo.PNG"] {
        assert!(check_logo_url(good).is_ok(), "{good:?} should be accepted");
    }
    for bad in [
        "ftp://acme.com/logo.png",
        "https://acme.com/logo.gif",
        "logo.png",
    ] {
        assert!(check_logo_url(bad).is_err(), "{bad:?} should be refused");
    }

    let err = from_flags(&parse(&["--logo-url", "https://acme.com/logo.gif"])).expect_err("gif");
    assert!(fix_of(&err).contains("--logo-url"), "{}", fix_of(&err));
}

#[test]
fn essential_is_restricted_when_arguments_are_parsed() {
    let err = Harness::try_parse_from(["init", "--essential", "No"])
        .err()
        .expect("rejected by clap");
    let message = err.to_string();
    assert!(message.contains("Always"), "{message}");
    assert!(message.contains("OnApply"), "{message}");
}

#[test]
fn the_four_identity_flags_are_required_by_the_parser() {
    // Required to the parser, so the engine can ask for whichever are missing
    // when prompting is on and report them all when it is off.
    let err = Harness::try_parse_from(["init", "--provider-id", "acme.com"])
        .err()
        .expect("missing names are refused");
    let message = err.to_string();
    assert!(message.contains("--provider-name"), "{message}");
    assert!(message.contains("--service-id"), "{message}");
    assert!(message.contains("--service-name"), "{message}");
}

#[test]
fn the_identity_flags_are_checked_as_they_are_read() {
    for bad in ["../x.com", "acme", "ACME.com"] {
        let err = Harness::try_parse_from(["init", "--provider-id", bad])
            .err()
            .expect("bad provider id");
        assert!(err.to_string().contains("domain name"), "{bad}: {err}");
    }
    let err = Harness::try_parse_from(["init", "--service-id", "Bad Id"])
        .err()
        .expect("bad service id");
    assert!(err.to_string().contains("letters, digits"), "{err}");
}

#[test]
fn the_command_needs_no_sign_in_and_handles_dry_run_itself() {
    let spec = command().spec;
    let meta = spec.metadata();
    assert!(spec.auth.is_none(), "init only writes a local file");
    assert!(meta.scopes.is_empty(), "{:?}", meta.scopes);
    assert!(meta.dry_run_prompt);
    assert!(meta.handles_dry_run);
}

#[test]
fn the_result_renders_through_the_view() {
    let dir = tempfile::tempdir().expect("tempdir");
    let template = from_flags(&parse(&["--add-a", "value=ip"])).expect("template");
    let result = write_template(&template, dir.path(), false, false).expect("written");

    let rendered = cli_engine::preview_human_view(&result.data, &view_columns());

    assert!(rendered.contains("acme.com.email.json"), "{rendered}");
    assert!(rendered.contains("Action: created"), "{rendered}");
    assert!(rendered.contains("Records: 1"), "{rendered}");
}
