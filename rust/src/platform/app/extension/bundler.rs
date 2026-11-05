use std::path::{Path, PathBuf};

use sha2::Digest as _;

use super::runtime_wrapper::{
    absolute_entry_path, create_ui_extension_runtime_wrapper,
    should_use_ui_extension_runtime_wrapper,
};
use super::types::{BundleCleanup, BundleOptions, BundleResult, ExtensionType};

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

/// Prefer `extensionDir/tsconfig.json`, else `repoRoot/tsconfig.json`.
fn resolve_tsconfig(extension_dir: &Path, repo_root: &Path) -> Option<PathBuf> {
    let local = extension_dir.join("tsconfig.json");
    if local.is_file() {
        return Some(local);
    }
    let root = repo_root.join("tsconfig.json");
    if root.is_file() {
        return Some(root);
    }
    None
}

/// Sanitize an extension handle/name for use in filenames.
///
/// Dots are rewritten so handles like `.` / `..` cannot make `Path::join`
/// resolve to the temp root or its parent.
fn sanitize_extension_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '@' | '!' | '.' => '-',
            c if c.is_whitespace() => '-',
            c => c,
        })
        .collect();
    let cleaned = sanitized
        .to_ascii_lowercase()
        .trim_matches(|c: char| c == '-' || c.is_whitespace())
        .chars()
        .take(100)
        .collect::<String>();
    if cleaned.is_empty() {
        "extension".to_owned()
    } else {
        cleaned
    }
}

/// UTC timestamp `yyyymmddHHMMss`.
pub(crate) fn format_timestamp(now: chrono::DateTime<chrono::Utc>) -> String {
    now.format("%Y%m%d%H%M%S").to_string()
}

fn short_hash(full_hash: &str) -> &str {
    full_hash.get(..6).unwrap_or(full_hash)
}

/// `{sanitized}-{version}-{timestamp}-{hash}.mjs`
fn build_artifact_name(name: &str, version: Option<&str>, timestamp: &str, hash: &str) -> String {
    format!(
        "{}-{}-{}-{}.mjs",
        sanitize_extension_name(name),
        version.unwrap_or("0.0.0"),
        timestamp,
        hash
    )
}

fn create_temp_directory(repo_root: &Path, timestamp: &str) -> PathBuf {
    let repo_name = repo_root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("repo");
    // Include PID so two deploys in the same second don't share a temp tree.
    let pid = std::process::id();
    std::env::temp_dir()
        .join("gd-cli")
        .join(repo_name)
        .join(format!("deploy-{timestamp}-{pid}"))
}

pub async fn bundle_extension(
    source_path: &Path,
    ext_type: ExtensionType,
    ext_dir: &Path,
    options: BundleOptions<'_>,
) -> Result<BundleResult, String> {
    let esbuild = find_esbuild();
    let timestamp = options
        .timestamp
        .map(str::to_owned)
        .unwrap_or_else(|| format_timestamp(chrono::Utc::now()));

    let temp_root = create_temp_directory(options.repo_root, &timestamp);
    // Clean the temp tree on any early return; disarmed only on success.
    let cleanup = BundleCleanup::for_dir(temp_root.clone());
    let extension_temp_dir = temp_root.join(sanitize_extension_name(options.name));
    tokio::fs::create_dir_all(&extension_temp_dir)
        .await
        .map_err(|e| format!("failed to create temp dir: {e}"))?;

    let mut build_entry = source_path.to_owned();
    if should_use_ui_extension_runtime_wrapper(ext_type) {
        let abs_source = absolute_entry_path(source_path).await;
        let wrapper_path = extension_temp_dir.join("ui-extension-runtime-entry.ts");
        let wrapper = create_ui_extension_runtime_wrapper(&abs_source);
        tokio::fs::write(&wrapper_path, wrapper)
            .await
            .map_err(|e| format!("failed to write UI runtime wrapper: {e}"))?;
        build_entry = wrapper_path;
    }

    let out_dir = extension_temp_dir.join("out");
    tokio::fs::create_dir_all(&out_dir)
        .await
        .map_err(|e| format!("failed to create out dir: {e}"))?;

    let mut args: Vec<String> = vec![
        build_entry.to_string_lossy().into_owned(),
        "--bundle".to_owned(),
        "--minify".to_owned(),
        "--sourcemap=external".to_owned(),
        format!("--outdir={}", out_dir.display()),
        "--out-extension:.js=.mjs".to_owned(),
        "--log-level=silent".to_owned(),
        "--external:node:*".to_owned(),
        "--external:@wsblocks/*".to_owned(),
    ];

    if let Some(tsconfig) = resolve_tsconfig(ext_dir, options.repo_root) {
        args.push(format!("--tsconfig={}", tsconfig.display()));
    }

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
        return Err(format!("esbuild failed: {stderr}"));
    }

    let mjs_path = find_output_mjs(&out_dir).await?;
    let map_path = {
        let candidate = PathBuf::from(format!("{}.map", mjs_path.display()));
        candidate.is_file().then_some(candidate)
    };

    let raw_bytes = tokio::fs::read(&mjs_path)
        .await
        .map_err(|e| format!("failed to read bundle output: {e}"))?;

    // Strip sourcemap comment before hashing; size/bytes include the
    // re-appended `sourceMappingURL` footer (matches TS behavior).
    let content = String::from_utf8_lossy(&raw_bytes);
    let stripped = strip_sourcemap_comment(&content);
    let sha256 = sha256_hex(stripped.as_bytes());
    let hash = short_hash(&sha256).to_owned();
    let artifact_name = build_artifact_name(options.name, options.version, &timestamp, &hash);

    let mut bundle_content = stripped;
    let mut sourcemap_path = None;
    if let Some(map_src) = map_path {
        let map_name = format!("{artifact_name}.map");
        bundle_content.push_str(&format!("\n//# sourceMappingURL={map_name}\n"));
        let map_dest = extension_temp_dir.join(&map_name);
        tokio::fs::copy(&map_src, &map_dest)
            .await
            .map_err(|e| format!("failed to write sourcemap: {e}"))?;
        sourcemap_path = Some(map_dest);
    }

    let artifact_path = extension_temp_dir.join(&artifact_name);
    tokio::fs::write(&artifact_path, bundle_content.as_bytes())
        .await
        .map_err(|e| format!("failed to write artifact: {e}"))?;

    let size = bundle_content.len() as u64;
    cleanup.disarm();
    Ok(BundleResult {
        bytes: bundle_content.into_bytes(),
        sha256,
        artifact_name,
        artifact_path,
        size,
        sourcemap_path,
        temp_dir: temp_root,
    })
}

/// Find the single `.mjs` output under `dir`. Fails if zero or multiple are present
/// (code-splitting / multi-chunk outputs are not supported).
async fn find_output_mjs(dir: &Path) -> Result<PathBuf, String> {
    let mut read_dir = tokio::fs::read_dir(dir)
        .await
        .map_err(|e| format!("failed to read temp dir: {e}"))?;
    let mut found = Vec::new();
    loop {
        match read_dir.next_entry().await {
            Ok(Some(entry)) => {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("mjs") {
                    found.push(path);
                }
            }
            Ok(None) => break,
            Err(e) => return Err(format!("failed to read temp dir entry: {e}")),
        }
    }
    match found.as_slice() {
        [only] => Ok(only.clone()),
        [] => Err("esbuild produced no .mjs output file".to_owned()),
        many => {
            let list = many
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            Err(format!(
                "esbuild produced multiple .mjs outputs (code splitting not supported): {list}"
            ))
        }
    }
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
    // Artifact naming / tsconfig
    // -----------------------------------------------------------------------

    #[test]
    fn sanitize_extension_name_scoped_and_special_chars() {
        assert_eq!(
            sanitize_extension_name("@scoped/extension"),
            "scoped-extension"
        );
        assert_eq!(sanitize_extension_name("My Extension!"), "my-extension");
        assert_eq!(sanitize_extension_name("@@@"), "extension");
        assert_eq!(sanitize_extension_name("."), "extension");
        assert_eq!(sanitize_extension_name(".."), "extension");
        assert_eq!(sanitize_extension_name("my.widget"), "my-widget");
    }

    #[test]
    fn sanitize_extension_name_dot_handles_stay_under_temp_root() {
        let temp_root = Path::new("/tmp/gd-cli/repo/deploy-ts-1");
        for handle in [".", ".."] {
            let joined = temp_root.join(sanitize_extension_name(handle));
            assert!(
                joined.starts_with(temp_root),
                "handle {handle:?} escaped temp root: {joined:?}"
            );
            assert_ne!(
                joined, temp_root,
                "handle {handle:?} collapsed to temp root"
            );
            assert_ne!(
                joined,
                temp_root.parent().expect("parent"),
                "handle {handle:?} resolved to temp parent"
            );
        }
    }

    #[test]
    fn build_artifact_name_uses_handle_version_timestamp_hash() {
        let name = build_artifact_name(
            "@scoped/extension",
            Some("1.0.0"),
            "20250128143022",
            "a3b2c1",
        );
        assert_eq!(name, "scoped-extension-1.0.0-20250128143022-a3b2c1.mjs");
    }

    #[test]
    fn build_artifact_name_defaults_missing_version() {
        let name = build_artifact_name("widget", None, "20250128143022", "abcdef");
        assert_eq!(name, "widget-0.0.0-20250128143022-abcdef.mjs");
    }

    #[test]
    fn format_timestamp_is_utc_compact() {
        use chrono::TimeZone;
        let ts = chrono::Utc
            .with_ymd_and_hms(2025, 1, 28, 14, 30, 22)
            .single()
            .expect("valid utc");
        assert_eq!(format_timestamp(ts), "20250128143022");
    }

    #[test]
    fn resolve_tsconfig_prefers_extension_local_over_repo_root() {
        let root = tempfile::tempdir().expect("temp root");
        let ext = root.path().join("ext");
        std::fs::create_dir_all(&ext).expect("ext dir");
        std::fs::write(root.path().join("tsconfig.json"), "{}").expect("root tsconfig");
        std::fs::write(ext.join("tsconfig.json"), "{}").expect("local tsconfig");
        let resolved = resolve_tsconfig(&ext, root.path()).expect("local");
        assert_eq!(resolved, ext.join("tsconfig.json"));
    }

    #[test]
    fn resolve_tsconfig_falls_back_to_repo_root() {
        let root = tempfile::tempdir().expect("temp root");
        let ext = root.path().join("ext");
        std::fs::create_dir_all(&ext).expect("ext dir");
        std::fs::write(root.path().join("tsconfig.json"), "{}").expect("root tsconfig");
        let resolved = resolve_tsconfig(&ext, root.path()).expect("root");
        assert_eq!(resolved, root.path().join("tsconfig.json"));
    }

    #[test]
    fn create_temp_directory_includes_pid() {
        let root = Path::new("/tmp/my-app");
        let dir = create_temp_directory(root, "20250731120000");
        let name = dir.file_name().and_then(|s| s.to_str()).expect("dir name");
        assert!(
            name.starts_with("deploy-20250731120000-"),
            "unexpected: {name}"
        );
        assert!(
            name.ends_with(&format!("-{}", std::process::id())),
            "unexpected: {name}"
        );
    }

    #[tokio::test]
    async fn find_output_mjs_rejects_multiple_chunks() {
        let dir = tempfile::tempdir().expect("temp");
        std::fs::write(dir.path().join("a.mjs"), "a").expect("a");
        std::fs::write(dir.path().join("b.mjs"), "b").expect("b");
        let err = find_output_mjs(dir.path())
            .await
            .expect_err("multiple .mjs");
        assert!(err.contains("multiple .mjs"), "unexpected error: {err}");
    }

    #[tokio::test]
    async fn find_output_mjs_returns_the_single_file() {
        let dir = tempfile::tempdir().expect("temp");
        let path = dir.path().join("bundle.mjs");
        std::fs::write(&path, "export {}").expect("write");
        let found = find_output_mjs(dir.path()).await.expect("one .mjs");
        assert_eq!(found, path);
    }

    /// Integration check against a minimal TS fixture when esbuild is on PATH
    /// (or under a nearby node_modules/.bin). Skipped otherwise so CI without
    /// Node still passes unit tests.
    #[tokio::test]
    async fn bundle_simple_blocks_fixture_when_esbuild_available() {
        let esbuild = find_esbuild();
        if tokio::process::Command::new(&esbuild)
            .arg("--version")
            .output()
            .await
            .map(|o| !o.status.success())
            .unwrap_or(true)
        {
            // esbuild not installed — unit coverage above still runs.
            return;
        }

        let root = tempfile::tempdir().expect("temp root");
        let src_dir = root.path().join("src");
        std::fs::create_dir_all(&src_dir).expect("src");
        let entry = src_dir.join("index.ts");
        std::fs::write(
            &entry,
            r#"export const name = "simple-extension";
export function handler() { return { success: true }; }
"#,
        )
        .expect("entry");

        let bundle = bundle_extension(
            &entry,
            ExtensionType::Blocks,
            root.path(),
            BundleOptions {
                name: "simple-extension",
                version: Some("1.0.0"),
                repo_root: root.path(),
                timestamp: Some("20250128143022"),
            },
        )
        .await
        .expect("bundle");
        let _cleanup = BundleCleanup::new(&bundle);

        assert!(
            bundle
                .artifact_name
                .starts_with("simple-extension-1.0.0-20250128143022-"),
            "artifact name: {}",
            bundle.artifact_name
        );
        assert!(bundle.artifact_name.ends_with(".mjs"));
        assert!(bundle.artifact_path.is_file());
        assert!(bundle.size > 0);
        assert_eq!(bundle.sha256.len(), 64);
        let content = String::from_utf8_lossy(&bundle.bytes);
        assert!(
            content.contains("simple-extension") || content.contains("success"),
            "bundle should retain exported strings: {content}"
        );
    }
}
