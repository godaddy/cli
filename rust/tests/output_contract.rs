use std::process::{Command, Output};

use serde_json::Value;

fn run_gddy(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gddy"))
        .args(args)
        .env("GDDY_ENV", "ote")
        .output()
        .expect("gddy should execute")
}

#[test]
fn success_uses_public_envelope_on_stdout() {
    let output = run_gddy(&["env", "get"]);
    assert!(output.status.success());

    let actual: Value = serde_json::from_slice(&output.stdout).expect("stdout should contain JSON");
    assert_eq!(actual["ok"], true);
    assert_eq!(actual["command"], "gddy env get");
    assert!(actual.get("result").is_some());
    assert!(actual["next_actions"].is_array());
    assert_eq!(
        actual
            .as_object()
            .expect("object")
            .keys()
            .collect::<Vec<_>>(),
        vec!["command", "next_actions", "ok", "result"]
    );
}

#[test]
fn failure_uses_public_envelope_on_stdout_and_preserves_exit_code() {
    let output = run_gddy(&["--env", "not-an-environment", "env", "get"]);
    assert!(!output.status.success());

    let actual: Value = serde_json::from_slice(&output.stdout).expect("stdout should contain JSON");
    assert_eq!(actual["ok"], false);
    assert_eq!(actual["command"], "gddy env get");
    assert_eq!(
        actual
            .as_object()
            .expect("object")
            .keys()
            .collect::<Vec<_>>(),
        vec!["command", "error", "next_actions", "ok"]
    );
    assert!(actual["error"]["code"].is_string());
    assert!(actual["error"]["message"].is_string());
    assert!(!String::from_utf8_lossy(&output.stderr).contains(r#""ok""#));
}

#[test]
fn help_remains_plain_text() {
    let output = run_gddy(&["--help"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Usage:"));
}

#[test]
fn version_remains_plain_text() {
    let output = run_gddy(&["--version"]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("gddy version "));
    assert!(serde_json::from_slice::<Value>(&output.stdout).is_err());
}
