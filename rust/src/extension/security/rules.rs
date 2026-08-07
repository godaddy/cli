//! SEC101–SEC115 rule data.
//! Ported from src/core/security/rules/bundle/ in the TypeScript CLI.

use crate::extension::types::Severity;

pub(super) struct RuleDef {
    pub(super) id: &'static str,
    pub(super) severity: Severity,
    pub(super) description: &'static str,
    /// Main detection patterns.
    pub(super) patterns: &'static [&'static str],
    /// Two-pass signal patterns: if non-empty and none match, skip this rule.
    pub(super) signal_patterns: &'static [&'static str],
}

pub(super) static RULE_DEFS: &[RuleDef] = &[
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
        patterns: &[r#"\bdebugger\b"#],
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
