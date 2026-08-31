//! SEC011 — suspicious package.json lifecycle scripts (warn).

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::extension::{Finding, Severity};

use super::types::ScanError;

const LIFECYCLE_SCRIPTS: &[&str] = &["install", "postinstall", "preinstall"];

struct SuspiciousPattern {
    name: &'static str,
    reason: &'static str,
    regex: Regex,
}

static PATTERNS: LazyLock<Vec<SuspiciousPattern>> = LazyLock::new(|| {
    [
        (
            "curl",
            r"(?i)\bcurl\b",
            "Download tool that can fetch remote payloads",
        ),
        (
            "wget",
            r"(?i)\bwget\b",
            "Download tool that can fetch remote payloads",
        ),
        (
            "bash -c",
            r"(?i)\bbash\s+-c\b",
            "Arbitrary command execution via bash",
        ),
        (
            "sh -c",
            r"(?i)\bsh\s+-c\b",
            "Arbitrary command execution via shell",
        ),
        (
            "powershell -enc",
            r"(?i)\bpowershell\s+-enc\b",
            "Encoded PowerShell command",
        ),
        ("nc", r"(?i)\bnc\b", "Network utility in lifecycle script"),
        (
            "mkfifo",
            r"(?i)\bmkfifo\b",
            "Named pipe creation in lifecycle script",
        ),
        (
            "eval",
            r"(?i)\beval\b",
            "Dynamic evaluation in lifecycle script",
        ),
        (
            "exec",
            r"(?i)\bexec\b",
            "Command execution in lifecycle script",
        ),
    ]
    .into_iter()
    .map(|(name, pattern, reason)| SuspiciousPattern {
        name,
        reason,
        regex: Regex::new(pattern).expect("valid SEC011 pattern"),
    })
    .collect()
});

/// Scan `package.json` lifecycle scripts.
///
/// Missing `package.json` is ignored. Other read/parse failures bubble up so the
/// pre-bundle scan can fail closed.
pub fn scan_package_scripts(package_json: &Path) -> Result<Vec<Finding>, ScanError> {
    let content = match std::fs::read_to_string(package_json) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(ScanError::Read {
                path: package_json.display().to_string(),
                source,
            });
        }
    };
    let value: serde_json::Value =
        serde_json::from_str(&content).map_err(|source| ScanError::InvalidPackageJson {
            path: package_json.display().to_string(),
            source,
        })?;
    let Some(scripts) = value.get("scripts").and_then(|s| s.as_object()) else {
        return Ok(Vec::new());
    };

    let mut findings = Vec::new();
    let file = package_json.display().to_string();
    for script_name in LIFECYCLE_SCRIPTS {
        let Some(script) = scripts.get(*script_name).and_then(|v| v.as_str()) else {
            continue;
        };
        for pat in PATTERNS.iter() {
            if pat.regex.is_match(script) {
                findings.push(Finding {
                    rule_id: "SEC011",
                    severity: Severity::Warn,
                    message: format!(
                        "Suspicious {} pattern ({}) in {} script: {}",
                        pat.name, pat.reason, script_name, script
                    ),
                    file: file.clone(),
                    line: 0,
                    col: 0,
                    snippet: String::new(),
                });
                break;
            }
        }
    }
    Ok(findings)
}
