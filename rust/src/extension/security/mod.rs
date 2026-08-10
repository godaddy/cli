use std::sync::OnceLock;

use super::types::{Finding, Severity};

mod rules;
#[cfg(test)]
mod tests_sec101_108;
#[cfg(test)]
mod tests_sec109_115;

use rules::RULE_DEFS;

struct CompiledRule {
    id: &'static str,
    severity: Severity,
    description: &'static str,
    patterns: Vec<fancy_regex::Regex>,
    signal_patterns: Vec<fancy_regex::Regex>,
}

static COMPILED: OnceLock<Vec<CompiledRule>> = OnceLock::new();

fn compiled_rules() -> &'static [CompiledRule] {
    COMPILED.get_or_init(|| {
        RULE_DEFS
            .iter()
            .map(|def| CompiledRule {
                id: def.id,
                severity: def.severity,
                description: def.description,
                patterns: def
                    .patterns
                    .iter()
                    .map(|p| fancy_regex::Regex::new(p).expect("invalid bundle rule pattern"))
                    .collect(),
                signal_patterns: def
                    .signal_patterns
                    .iter()
                    .map(|p| fancy_regex::Regex::new(p).expect("invalid bundle signal pattern"))
                    .collect(),
            })
            .collect()
    })
}

// ---------------------------------------------------------------------------
// Scanner helpers
// ---------------------------------------------------------------------------

fn line_number(content: &str, byte_offset: usize) -> usize {
    content[..byte_offset]
        .bytes()
        .filter(|&b| b == b'\n')
        .count()
        + 1
}

fn extract_snippet(content: &str, byte_offset: usize) -> String {
    let line_start = content[..byte_offset].rfind('\n').map_or(0, |i| i + 1);
    let line_end = content[byte_offset..]
        .find('\n')
        .map_or(content.len(), |i| byte_offset + i);
    let line = &content[line_start..line_end];
    let pos = byte_offset - line_start;
    let ctx = 25_usize;
    let start = pos.saturating_sub(ctx);
    let end = (pos + ctx).min(line.len());
    line[start..end].trim().to_owned()
}

// ---------------------------------------------------------------------------
// Public scanner API
// ---------------------------------------------------------------------------

pub fn scan_bundle(content: &str, file_path: &str) -> Vec<Finding> {
    let mut findings = Vec::new();

    for rule in compiled_rules() {
        if !rule.signal_patterns.is_empty() {
            let signal_found = rule
                .signal_patterns
                .iter()
                .any(|re| re.is_match(content).unwrap_or(false));
            if !signal_found {
                continue;
            }
            for re in rule.signal_patterns.iter().chain(rule.patterns.iter()) {
                for m in re.find_iter(content).filter_map(|m| m.ok()) {
                    findings.push(Finding {
                        rule_id: rule.id,
                        severity: rule.severity,
                        message: rule.description,
                        file: file_path.to_owned(),
                        line: line_number(content, m.start()),
                        snippet: extract_snippet(content, m.start()),
                    });
                }
            }
        } else {
            for re in &rule.patterns {
                for m in re.find_iter(content).filter_map(|m| m.ok()) {
                    findings.push(Finding {
                        rule_id: rule.id,
                        severity: rule.severity,
                        message: rule.description,
                        file: file_path.to_owned(),
                        line: line_number(content, m.start()),
                        snippet: extract_snippet(content, m.start()),
                    });
                }
            }
        }
    }

    findings.sort_by_key(|f| f.line);
    findings
}

pub fn is_blocked(findings: &[Finding]) -> bool {
    findings.iter().any(|f| f.severity == Severity::Block)
}

// ---------------------------------------------------------------------------
// Tests: line_number/extract_snippet/is_blocked/scan_bundle core behavior.
// Per-rule detection tests live in tests_sec101_108.rs / tests_sec109_115.rs
// — split purely to stay under the file-size limit (docs/code-structure.md).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // line_number helper
    // -----------------------------------------------------------------------

    #[test]
    fn line_number_first_line_start() {
        assert_eq!(line_number("hello", 0), 1);
    }

    #[test]
    fn line_number_first_line_end() {
        assert_eq!(line_number("hello", 4), 1);
    }

    #[test]
    fn line_number_second_line() {
        let s = "line1\nline2";
        assert_eq!(line_number(s, 6), 2);
    }

    #[test]
    fn line_number_third_line() {
        let s = "a\nb\nc";
        assert_eq!(line_number(s, 4), 3);
    }

    #[test]
    fn line_number_at_newline_char() {
        let s = "a\nb";
        assert_eq!(line_number(s, 1), 1);
    }

    #[test]
    fn line_number_crlf_counts_only_lf() {
        let s = "line1\r\nline2";
        assert_eq!(line_number(s, 7), 2);
    }

    #[test]
    fn line_number_empty_offset() {
        assert_eq!(line_number("", 0), 1);
    }

    // -----------------------------------------------------------------------
    // extract_snippet helper
    // -----------------------------------------------------------------------

    #[test]
    fn extract_snippet_short_line() {
        let s = r#"const x = eval("bad");"#;
        let snip = extract_snippet(s, 10);
        assert!(snip.contains("eval"), "snippet: {snip}");
    }

    #[test]
    fn extract_snippet_truncates_long_line() {
        let long = format!("{}MATCH{}", "a".repeat(40), "b".repeat(40));
        let pos = 40;
        let snip = extract_snippet(&long, pos);
        assert!(snip.contains("MATCH"), "snippet: {snip}");
        assert!(snip.len() <= 60, "snippet too long: {snip}");
    }

    #[test]
    fn extract_snippet_does_not_cross_newlines() {
        let s = "line1\neval(\"bad\")\nline3";
        let snip = extract_snippet(s, 6);
        assert!(snip.contains("eval"), "snippet: {snip}");
        assert!(!snip.contains("line1"), "leaked into prev line: {snip}");
        assert!(!snip.contains("line3"), "leaked into next line: {snip}");
    }

    #[test]
    fn extract_snippet_at_file_start() {
        let s = "eval(\"x\") + 1";
        let snip = extract_snippet(s, 0);
        assert!(snip.contains("eval"), "snippet: {snip}");
    }

    // -----------------------------------------------------------------------
    // is_blocked
    // -----------------------------------------------------------------------

    #[test]
    fn is_blocked_empty_findings() {
        assert!(!is_blocked(&[]));
    }

    #[test]
    fn is_blocked_warn_only_returns_false() {
        let findings = vec![Finding {
            rule_id: "SEC108",
            severity: Severity::Warn,
            message: "test",
            file: "f.mjs".to_owned(),
            line: 1,
            snippet: String::new(),
        }];
        assert!(!is_blocked(&findings));
    }

    #[test]
    fn is_blocked_block_finding_returns_true() {
        let findings = vec![Finding {
            rule_id: "SEC101",
            severity: Severity::Block,
            message: "test",
            file: "f.mjs".to_owned(),
            line: 1,
            snippet: String::new(),
        }];
        assert!(is_blocked(&findings));
    }

    #[test]
    fn is_blocked_mixed_returns_true() {
        let findings = vec![
            Finding {
                rule_id: "SEC108",
                severity: Severity::Warn,
                message: "warn",
                file: "f.mjs".to_owned(),
                line: 1,
                snippet: String::new(),
            },
            Finding {
                rule_id: "SEC101",
                severity: Severity::Block,
                message: "block",
                file: "f.mjs".to_owned(),
                line: 2,
                snippet: String::new(),
            },
        ];
        assert!(is_blocked(&findings));
    }

    // -----------------------------------------------------------------------
    // scan_bundle: general
    // -----------------------------------------------------------------------

    #[test]
    fn scan_clean_content_no_findings() {
        let findings = scan_bundle("const x = 1 + 2;\nconsole.log(x);\n", "clean.mjs");
        assert!(findings.is_empty(), "expected no findings: {findings:?}");
    }

    #[test]
    fn scan_results_sorted_by_line() {
        let content = "eval(\"first\");\nclean;\neval(\"second\");";
        let findings = scan_bundle(content, "test.mjs");
        let lines: Vec<usize> = findings.iter().map(|f| f.line).collect();
        let mut sorted = lines.clone();
        sorted.sort_unstable();
        assert_eq!(lines, sorted, "findings not sorted by line");
    }
}
