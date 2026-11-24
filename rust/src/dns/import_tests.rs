use super::*;
use httpmock::prelude::*;

const ZONE_PATH: &str = "/v3/domains/zones/example.com/dns-records";

fn client_for(server: &MockServer) -> domains_client::Client {
    domains_client::client_with_builder(
        reqwest::Client::builder(),
        &server.base_url(),
        "Bearer tok",
        "req-1",
    )
    .expect("build client")
}

fn zone(text: &str) -> Imported {
    from_zone_text(text, "example.com").expect("zone reads")
}

fn api_record(id: &str, ty: &str, name: &str, data: &str, ttl: i64) -> types::DnsRecord {
    serde_json::from_value(json!({
        "recordId": id, "type": ty, "name": name, "data": data, "ttl": ttl
    }))
    .expect("record")
}

async fn run(
    server: &MockServer,
    imported: Imported,
    existing: &[types::DnsRecord],
    replace: bool,
    dry_run: bool,
) -> Result<Report, CliCoreError> {
    run_import(
        &client_for(server),
        "example.com",
        imported,
        existing,
        replace,
        dry_run,
        false,
    )
    .await
}

fn records(report: &Report) -> &Vec<Value> {
    report.payload["records"].as_array().expect("records")
}

#[tokio::test]
async fn a_real_import_creates_each_missing_record_and_reports_them() {
    let server = MockServer::start_async().await;
    let www = server
        .mock_async(|when, then| {
            when.method(POST).path(ZONE_PATH).json_body(json!({
                "type": "A", "name": "www", "data": "192.0.2.1", "ttl": 600
            }));
            then.status(201).json_body(json!({
                "type": "A", "name": "www", "data": "192.0.2.1", "ttl": 600, "recordId": "n1"
            }));
        })
        .await;
    let mail = server
        .mock_async(|when, then| {
            when.method(POST).path(ZONE_PATH).json_body(json!({
                "type": "MX", "name": "@", "data": "mail.example.com.", "priority": 10, "ttl": 3600
            }));
            then.status(201).json_body(json!({
                "type": "MX", "name": "@", "data": "mail.example.com.", "priority": 10,
                "ttl": 3600, "recordId": "n2"
            }));
        })
        .await;

    let report = run(
        &server,
        zone("www 600 A 192.0.2.1\n@ 3600 MX 10 mail\n"),
        &[],
        false,
        false,
    )
    .await
    .expect("imports");

    www.assert_async().await;
    mail.assert_async().await;
    assert_eq!(report.payload["created"], 2);
    assert_eq!(report.payload["failed"], 0);
    assert_eq!(report.payload["action"], "import");
    let entries = records(&report);
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|e| e["action"] == "create"));
    assert_eq!(entries[0]["type"], "A");
    assert_eq!(entries[0]["name"], "www");
    assert_eq!(entries[0]["recordId"], "n1");
}

#[tokio::test]
async fn importing_what_is_already_there_writes_nothing() {
    let server = MockServer::start_async().await;
    let any_write = server
        .mock_async(|when, then| {
            when.method(POST).path(ZONE_PATH);
            then.status(500);
        })
        .await;
    let existing = [api_record("r1", "A", "www", "192.0.2.1", 600)];

    let report = run(
        &server,
        zone("www 600 A 192.0.2.1\n"),
        &existing,
        false,
        false,
    )
    .await
    .expect("imports");

    assert_eq!(any_write.calls_async().await, 0);
    assert_eq!(report.payload["unchanged"], 1);
    assert_eq!(report.payload["action"], "nothing to change");
}

#[tokio::test]
async fn a_ttl_only_difference_replaces_the_record_in_place() {
    let server = MockServer::start_async().await;
    let put = server
        .mock_async(|when, then| {
            when.method(PUT).path(format!("{ZONE_PATH}/r1"));
            then.status(200).json_body(json!({
                "type": "A", "name": "www", "data": "192.0.2.1", "ttl": 600, "recordId": "r1"
            }));
        })
        .await;
    let existing = [api_record("r1", "A", "www", "192.0.2.1", 3600)];

    let report = run(
        &server,
        zone("www 600 A 192.0.2.1\n"),
        &existing,
        false,
        false,
    )
    .await
    .expect("imports");

    put.assert_async().await;
    assert_eq!(report.payload["replaced"], 1);
    assert_eq!(report.payload["created"], 0);
}

#[tokio::test]
async fn replace_deletes_records_the_input_lacks_and_does_it_before_creating() {
    let server = MockServer::start_async().await;
    let delete = server
        .mock_async(|when, then| {
            when.method(DELETE).path(format!("{ZONE_PATH}/r9"));
            then.status(204);
        })
        .await;
    let create = server
        .mock_async(|when, then| {
            when.method(POST).path(ZONE_PATH);
            then.status(201).json_body(json!({
                "type": "CNAME", "name": "www", "data": "t.example.net.", "ttl": 3600, "recordId": "n1"
            }));
        })
        .await;
    // `www` is an A record now; the input makes it a CNAME. The A has to go first.
    let existing = [api_record("r9", "A", "www", "192.0.2.1", 3600)];

    let report = run(
        &server,
        zone("www CNAME t.example.net.\n"),
        &existing,
        true,
        false,
    )
    .await
    .expect("imports");

    delete.assert_async().await;
    create.assert_async().await;
    assert_eq!(report.payload["deleted"], 1);
    assert_eq!(report.payload["created"], 1);
}

#[tokio::test]
async fn without_replace_nothing_is_ever_deleted() {
    let server = MockServer::start_async().await;
    let delete = server
        .mock_async(|when, then| {
            when.method(DELETE);
            then.status(500);
        })
        .await;
    let existing = [api_record("r9", "A", "old", "192.0.2.9", 3600)];

    let report = run(&server, zone("new A 192.0.2.1\n"), &existing, false, true)
        .await
        .expect("previews");

    assert_eq!(delete.calls_async().await, 0);
    assert_eq!(report.payload["deleted"], 0);
}

#[tokio::test]
async fn a_dry_run_writes_nothing_and_reports_what_a_real_run_would() {
    let server = MockServer::start_async().await;
    let anything = server
        .mock_async(|when, then| {
            when.any_request();
            then.status(500);
        })
        .await;
    let existing = [
        api_record("r1", "A", "www", "192.0.2.1", 3600),
        api_record("r2", "A", "gone", "192.0.2.7", 3600),
    ];
    let input = "www 600 A 192.0.2.1\nnew A 192.0.2.2\n";

    let preview = run(&server, zone(input), &existing, true, true)
        .await
        .expect("previews");
    assert_eq!(anything.calls_async().await, 0, "a dry run makes no calls");
    assert_eq!(preview.payload["action"], "would import");
    assert_eq!(
        (
            preview.payload["created"].clone(),
            preview.payload["replaced"].clone(),
            preview.payload["deleted"].clone()
        ),
        (json!(1), json!(1), json!(1))
    );

    // The same input, really applied, reports the same actions per record.
    let real = MockServer::start_async().await;
    for (method, path, status, body) in [
        (
            PUT,
            format!("{ZONE_PATH}/r1"),
            200,
            json!({"type":"A","name":"www","data":"192.0.2.1","ttl":600,"recordId":"r1"}),
        ),
        (
            POST,
            ZONE_PATH.to_owned(),
            201,
            json!({"type":"A","name":"new","data":"192.0.2.2","ttl":3600,"recordId":"n1"}),
        ),
        (DELETE, format!("{ZONE_PATH}/r2"), 204, json!(null)),
    ] {
        real.mock_async(|when, then| {
            when.method(method).path(path);
            then.status(status).json_body(body);
        })
        .await;
    }
    let done = run(&real, zone(input), &existing, true, false)
        .await
        .expect("imports");
    let actions = |report: &Report| {
        let mut all: Vec<(String, String)> = records(report)
            .iter()
            .map(|e| (e["action"].to_string(), e["name"].to_string()))
            .collect();
        all.sort();
        all
    };
    assert_eq!(actions(&preview), actions(&done));
}

#[tokio::test]
async fn a_blocked_create_is_a_failure_in_the_preview_and_in_a_real_run() {
    let server = MockServer::start_async().await;
    let existing = [api_record("r1", "A", "www", "192.0.2.1", 3600)];

    let preview = run(
        &server,
        zone("www CNAME t.example.net.\n"),
        &existing,
        false,
        true,
    )
    .await
    .expect("previews");
    assert_eq!(preview.payload["failed"], 1);
    assert_eq!(records(&preview)[0]["action"], "fail");
    assert!(
        records(&preview)[0]["error"]
            .as_str()
            .is_some_and(|e| e.contains("can't coexist")),
        "{:?}",
        records(&preview)[0]
    );

    let err = run(
        &server,
        zone("www CNAME t.example.net.\n"),
        &existing,
        false,
        false,
    )
    .await
    .expect_err("a refused create fails the import");
    assert!(err.to_string().contains("1 of 1"), "{err}");
}

#[tokio::test]
async fn a_failed_write_is_reported_per_record_and_the_others_still_run() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST).path(ZONE_PATH).body_includes("192.0.2.1");
            then.status(422).json_body(json!({
                "name": "VALIDATION_ERROR", "message": "bad value", "details": []
            }));
        })
        .await;
    let ok = server
        .mock_async(|when, then| {
            when.method(POST).path(ZONE_PATH).body_includes("192.0.2.2");
            then.status(201).json_body(json!({
                "type": "A", "name": "b", "data": "192.0.2.2", "ttl": 3600, "recordId": "n2"
            }));
        })
        .await;

    let err = run(
        &server,
        zone("a A 192.0.2.1\nb A 192.0.2.2\n"),
        &[],
        false,
        false,
    )
    .await
    .expect_err("one failed");

    ok.assert_async().await;
    let message = err.to_string();
    assert!(message.contains("1 of 2"), "{message}");
    assert!(message.contains("✓ created A b"), "{message}");
    assert!(message.contains("✗ created A a"), "{message}");
    assert!(message.contains("safe"), "says a re-run is safe: {message}");
}

#[tokio::test]
async fn a_bad_record_stops_the_import_before_anything_is_written() {
    let server = MockServer::start_async().await;
    let anything = server
        .mock_async(|when, then| {
            when.any_request();
            then.status(500);
        })
        .await;

    // Checked while the input is read, before a client exists to call anything.
    let err = parse_input(
        "ok 600 A 192.0.2.1\nlow 100 A 192.0.2.2\nalso 200 A 192.0.2.3\n",
        InputFormat::Zone,
        "example.com",
    )
    .expect_err("ttl out of range");

    assert_eq!(anything.calls_async().await, 0);
    let message = err.to_string();
    assert!(message.contains("2 problem(s)"), "{message}");
    assert!(message.contains("line 2: the TTL 100"), "{message}");
    assert!(message.contains("line 3: the TTL 200"), "{message}");
    assert!(message.contains("nothing was imported"), "{message}");
}

#[tokio::test]
async fn skipped_ns_and_soa_become_warnings_with_the_delegations_named() {
    let server = MockServer::start_async().await;
    let report = run(
        &server,
        zone(
            "@ SOA ns1 admin 1 7200 900 1209600 300\n\
             @ NS ns1.example.com.\n\
             @ NS ns2.example.com.\n\
             sub NS ns.other.net.\n",
        ),
        &[],
        false,
        true,
    )
    .await
    .expect("previews");

    assert_eq!(report.payload["skipped"], 4);
    assert_eq!(report.warnings.len(), 2, "{:?}", report.warnings);
    assert!(
        report.warnings[0].contains("3 NS/SOA record(s)"),
        "{:?}",
        report.warnings
    );
    assert!(
        report.warnings[1].starts_with("line 4: the NS record for `sub`"),
        "{:?}",
        report.warnings
    );
}

#[test]
fn a_unicode_name_in_a_zone_file_imports_as_the_punycode_record_it_already_is() {
    // The zone file says `café`; the API holds `xn--caf-dma`. They are one record.
    let imported = zone("café 600 A 192.0.2.1\n");
    let existing = [api_record("r1", "A", "xn--caf-dma", "192.0.2.1", 600)];
    let mut desired = imported.desired;
    assert!(validate(&mut desired).is_empty());
    assert_eq!(desired[0].name, "xn--caf-dma");
    let plan = build_plan("example.com", desired, &existing, true).expect("plans");
    assert!(
        plan.groups
            .iter()
            .flat_map(|g| &g.actions)
            .all(|a| matches!(a, SetAction::Unchanged { .. })),
        "an unchanged record is left alone, even with --replace"
    );
}

#[test]
fn input_format_is_worked_out_from_the_content() {
    let json = r#"  [{"type":"A","name":"www","data":"192.0.2.1","ttl":600}]"#;
    let parsed = parse_input(json, InputFormat::Auto, "example.com").expect("json");
    assert_eq!(parsed.desired[0].at, "record 1");

    let parsed =
        parse_input("www 600 A 192.0.2.1\n", InputFormat::Auto, "example.com").expect("zone");
    assert_eq!(parsed.desired[0].at, "line 1");

    // An explicit format wins over what the content looks like.
    assert!(parse_input(json, InputFormat::Zone, "example.com").is_err());
    assert!(parse_input("www 600 A 192.0.2.1\n", InputFormat::Json, "example.com").is_err());
}

#[test]
fn a_long_list_of_problems_is_cut_short_and_counted() {
    let problems: Vec<String> = (1..=30).map(|n| format!("line {n}: bad")).collect();
    let message = input_problems(&problems).to_string();
    assert!(message.contains("30 problem(s)"), "{message}");
    assert!(message.contains("line 20: bad"), "{message}");
    assert!(!message.contains("line 21: bad"), "{message}");
    assert!(message.contains("and 10 more"), "{message}");
}

#[tokio::test]
async fn reading_a_file_and_a_missing_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("example.com.zone");
    std::fs::write(&path, "\u{feff}www 600 A 192.0.2.1\n").expect("write");

    let text = read_input(path.to_str().expect("utf-8 path"))
        .await
        .expect("reads");
    assert_eq!(
        text, "www 600 A 192.0.2.1\n",
        "a byte-order mark is dropped"
    );

    let err = read_input(
        dir.path()
            .join("missing.zone")
            .to_str()
            .expect("utf-8 path"),
    )
    .await
    .expect_err("missing");
    assert!(err.to_string().contains("could not open"), "{err}");

    std::fs::write(&path, [0xff, 0xfe, 0x00]).expect("write");
    let err = read_input(path.to_str().expect("utf-8 path"))
        .await
        .expect_err("not text");
    assert!(err.to_string().contains("not valid UTF-8"), "{err}");
}
