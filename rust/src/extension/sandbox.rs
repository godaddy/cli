use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Path normalization / sandboxing helpers
// ---------------------------------------------------------------------------

/// Lexically normalize `.` / `..` without requiring the path to exist on disk.
pub(super) fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(c) => out.push(c),
        }
    }
    out
}

pub(crate) fn repo_root_from_cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn invalid_handle_path(handle: &str) -> String {
    format!(
        "Invalid extension handle path: {handle}. \
         Extension directories must stay within ./extensions."
    )
}

fn invalid_source_path(extension_name: &str, source: &str) -> String {
    format!(
        "Invalid extension source path for '{extension_name}': {source}. \
         Source files must stay within the extension directory."
    )
}

/// True when `candidate` is `base` or a descendant.
///
/// Comparison is ASCII case-insensitive so manifests that mix handle casing
/// with a repo-relative `extensions/{handle}/…` source still resolve on
/// case-insensitive volumes (default macOS / Windows).
fn is_path_within(base: &Path, candidate: &Path) -> bool {
    strip_prefix_ignore_ascii_case(candidate, base).is_some()
}

/// Like [`Path::strip_prefix`], but component matching ignores ASCII case.
fn strip_prefix_ignore_ascii_case(path: &Path, prefix: &Path) -> Option<PathBuf> {
    let path = normalize_path(path);
    let prefix = normalize_path(prefix);
    let path_comps: Vec<_> = path.components().collect();
    let prefix_comps: Vec<_> = prefix.components().collect();
    if path_comps.len() < prefix_comps.len() {
        return None;
    }
    for (p, pre) in path_comps.iter().zip(prefix_comps.iter()) {
        if !components_eq_ignore_ascii_case(p, pre) {
            return None;
        }
    }
    let mut out = PathBuf::new();
    for component in &path_comps[prefix_comps.len()..] {
        out.push(component.as_os_str());
    }
    Some(out)
}

fn components_eq_ignore_ascii_case(
    a: &std::path::Component<'_>,
    b: &std::path::Component<'_>,
) -> bool {
    use std::path::Component;
    match (a, b) {
        (Component::Normal(a), Component::Normal(b)) => a.eq_ignore_ascii_case(b),
        _ => a == b,
    }
}

/// Walk `candidate` under `root`, matching each path segment against an existing
/// directory while ignoring ASCII case. Used so a handle like `Foo/Bar` still
/// finds `extensions/foo/bar` on case-sensitive volumes.
///
/// Falls back to `candidate` when any segment is missing (lexical path kept for
/// later "not found" errors).
fn dir_casing_on_disk(root: &Path, candidate: &Path) -> PathBuf {
    let Some(rel) = strip_prefix_ignore_ascii_case(candidate, root) else {
        return candidate.to_path_buf();
    };
    if rel.as_os_str().is_empty() {
        return root.to_path_buf();
    }
    if candidate.is_dir() {
        return candidate.to_path_buf();
    }

    use std::path::Component;
    let mut current = root.to_path_buf();
    for component in rel.components() {
        let Component::Normal(name) = component else {
            current.push(component.as_os_str());
            continue;
        };
        let exact = current.join(name);
        if exact.is_dir() {
            current = exact;
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&current) else {
            return candidate.to_path_buf();
        };
        let matched = entries.flatten().find_map(|entry| {
            (entry.file_name().eq_ignore_ascii_case(name) && entry.path().is_dir())
                .then(|| entry.path())
        });
        match matched {
            Some(path) => current = path,
            None => return candidate.to_path_buf(),
        }
    }
    normalize_path(&current)
}

/// When `path` exists, follow symlinks and ensure the real path stays under
/// `base`. Post-canonicalize containment is case-sensitive.
fn enforce_sandbox_realpath(
    base: &Path,
    path: &Path,
    outside_message: impl FnOnce() -> String,
) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let canon_base = std::fs::canonicalize(base)
        .map_err(|e| format!("failed to resolve path '{}': {e}", base.display()))?;
    let canon_path = std::fs::canonicalize(path)
        .map_err(|e| format!("failed to resolve path '{}': {e}", path.display()))?;
    if canon_path.strip_prefix(&canon_base).is_err() {
        return Err(outside_message());
    }
    Ok(())
}

/// Resolve and sandbox extension paths:
/// - `extensionDir` = `repoRoot/extensions/{handle}` (must stay under `extensions/`)
/// - `sourcePath` must stay under that directory
///
/// Also accepts a repo-relative `source` that already points inside the extension
/// dir (older Rust manifests that stored `extensions/{handle}/src/...`).
///
/// When paths exist on disk, containment is re-checked after canonicalization so
/// symlinks cannot escape the sandbox.
pub(crate) fn resolve_extension_paths(
    repo_root: &Path,
    handle: &str,
    source: &str,
    extension_name: &str,
) -> Result<(PathBuf, PathBuf), String> {
    let repo_root = if repo_root.is_absolute() {
        normalize_path(repo_root)
    } else {
        normalize_path(&repo_root_from_cwd().join(repo_root))
    };
    let extensions_root = normalize_path(&repo_root.join("extensions"));
    let extension_dir = normalize_path(&extensions_root.join(handle));
    if !is_path_within(&extensions_root, &extension_dir) {
        return Err(invalid_handle_path(handle));
    }
    let extension_dir = dir_casing_on_disk(&extensions_root, &extension_dir);

    let source_path = {
        let source_path = Path::new(source);
        if source_path.is_absolute() {
            normalize_path(source_path)
        } else {
            let from_repo = normalize_path(&repo_root.join(source));
            match strip_prefix_ignore_ascii_case(&from_repo, &extension_dir) {
                Some(rel) => normalize_path(&extension_dir.join(rel)),
                None => normalize_path(&extension_dir.join(source)),
            }
        }
    };
    if !is_path_within(&extension_dir, &source_path) {
        return Err(invalid_source_path(extension_name, source));
    }

    enforce_sandbox_realpath(&extensions_root, &extension_dir, || {
        invalid_handle_path(handle)
    })?;
    enforce_sandbox_realpath(&extension_dir, &source_path, || {
        invalid_source_path(extension_name, source)
    })?;

    Ok((extension_dir, source_path))
}

/// Ensure the resolved source path exists as a file.
pub(crate) fn require_extension_source_file(
    handle: &str,
    extension_name: &str,
    source_path: &Path,
) -> Result<(), String> {
    if source_path.is_file() {
        return Ok(());
    }
    Err(format!(
        "Extension source file not found for '{extension_name}': {}. \
         Expected a file under extensions/{handle}/ (e.g. src/index.ts).",
        source_path.display()
    ))
}

/// Validate handle/source against the deploy sandbox and return the source path
/// relative to `extensions/{handle}/` for writing into `godaddy.toml`.
///
/// Accepts either a handle-relative path (`src/index.ts`) or a repo-relative path
/// already under the extension dir (`extensions/{handle}/src/index.ts`). Requires
/// the resolved file to exist so `platform app add` fails before deploy.
pub(crate) fn normalize_extension_source_for_config(
    repo_root: &Path,
    handle: &str,
    source: &str,
    extension_name: &str,
) -> Result<String, String> {
    let (extension_dir, source_path) =
        resolve_extension_paths(repo_root, handle, source, extension_name)?;
    require_extension_source_file(handle, extension_name, &source_path)?;
    let relative = strip_prefix_ignore_ascii_case(&source_path, &extension_dir)
        .ok_or_else(|| invalid_source_path(extension_name, source))?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_path_within_accepts_descendants_and_rejects_siblings() {
        let base = Path::new("/repo/extensions");
        assert!(is_path_within(base, Path::new("/repo/extensions")));
        assert!(is_path_within(base, Path::new("/repo/extensions/widget")));
        assert!(is_path_within(
            base,
            Path::new("/repo/extensions/widget/src/index.ts")
        ));
        assert!(!is_path_within(base, Path::new("/repo/other")));
        assert!(!is_path_within(
            base,
            Path::new("/repo/extensions-extra/widget")
        ));
    }

    #[test]
    fn resolve_extension_paths_accepts_handle_relative_source() {
        let root = tempfile::tempdir().expect("temp root");
        let (ext_dir, source) =
            resolve_extension_paths(root.path(), "widget", "src/index.ts", "Widget").expect("ok");
        assert_eq!(
            ext_dir,
            normalize_path(&root.path().join("extensions/widget"))
        );
        assert_eq!(
            source,
            normalize_path(&root.path().join("extensions/widget/src/index.ts"))
        );
    }

    #[test]
    fn resolve_extension_paths_accepts_repo_relative_source_already_under_handle() {
        let root = tempfile::tempdir().expect("temp root");
        let (ext_dir, source) = resolve_extension_paths(
            root.path(),
            "widget",
            "extensions/widget/src/index.ts",
            "Widget",
        )
        .expect("ok");
        assert_eq!(
            ext_dir,
            normalize_path(&root.path().join("extensions/widget"))
        );
        assert_eq!(
            source,
            normalize_path(&root.path().join("extensions/widget/src/index.ts"))
        );
    }

    #[test]
    fn resolve_extension_paths_accepts_repo_relative_source_with_handle_case_mismatch() {
        let root = tempfile::tempdir().expect("temp root");
        let (ext_dir, source) = resolve_extension_paths(
            root.path(),
            "Widget",
            "extensions/widget/src/index.ts",
            "Widget",
        )
        .expect("ok");
        // Prefer the handle's casing for the extension dir / source prefix.
        assert_eq!(
            ext_dir,
            normalize_path(&root.path().join("extensions/Widget"))
        );
        assert_eq!(
            source,
            normalize_path(&root.path().join("extensions/Widget/src/index.ts"))
        );
    }

    #[test]
    fn resolve_extension_paths_keeps_the_casing_that_exists_on_disk() {
        let root = tempfile::tempdir().expect("temp root");
        let entry = root.path().join("extensions/widget/src/index.ts");
        std::fs::create_dir_all(entry.parent().expect("parent")).expect("mkdir");
        std::fs::write(&entry, "export {}").expect("write");

        let (ext_dir, source) = resolve_extension_paths(
            root.path(),
            "Widget",
            "extensions/widget/src/index.ts",
            "Widget",
        )
        .expect("ok");
        assert!(ext_dir.is_dir(), "should use the directory on disk");
        assert!(source.is_file(), "should resolve to the file on disk");

        let relative =
            normalize_extension_source_for_config(root.path(), "Widget", "src/index.ts", "Widget")
                .expect("normalize");
        assert_eq!(relative, "src/index.ts");
    }

    #[test]
    fn is_path_within_ignores_ascii_case() {
        assert!(is_path_within(
            Path::new("/repo/extensions/Widget"),
            Path::new("/repo/extensions/widget/src/index.ts")
        ));
        assert!(!is_path_within(
            Path::new("/repo/extensions/Widget"),
            Path::new("/repo/extensions/other/src/index.ts")
        ));
    }

    #[test]
    fn resolve_extension_paths_rejects_handle_escaping_extensions() {
        let root = tempfile::tempdir().expect("temp root");
        let err = resolve_extension_paths(root.path(), "../evil", "src/index.ts", "Evil")
            .expect_err("escape handle");
        assert!(
            err.contains("Invalid extension handle path"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn resolve_extension_paths_rejects_source_escaping_extension_dir() {
        let root = tempfile::tempdir().expect("temp root");
        let err = resolve_extension_paths(root.path(), "widget", "../../secret.ts", "Widget")
            .expect_err("escape source");
        assert!(
            err.contains("Invalid extension source path for 'Widget'"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn dir_casing_on_disk_walks_nested_handle_segments() {
        let root = tempfile::tempdir().expect("temp root");
        let nested = root.path().join("extensions/foo/bar");
        std::fs::create_dir_all(&nested).expect("mkdir");
        // Sibling that must NOT be chosen when resolving nested handle foo/Bar.
        std::fs::create_dir_all(root.path().join("extensions/bar")).expect("mkdir sibling");

        let extensions_root = normalize_path(&root.path().join("extensions"));
        let candidate = normalize_path(&extensions_root.join("Foo/Bar"));
        let resolved = dir_casing_on_disk(&extensions_root, &candidate);
        let resolved_canon = std::fs::canonicalize(&resolved).expect("resolved exists");
        let nested_canon = std::fs::canonicalize(&nested).expect("nested exists");
        assert_eq!(resolved_canon, nested_canon);
        assert_ne!(
            resolved_canon,
            std::fs::canonicalize(root.path().join("extensions/bar")).expect("sibling"),
            "must not pick the top-level extensions/bar sibling"
        );
    }

    #[test]
    fn resolve_extension_paths_nested_handle_does_not_pick_sibling() {
        let root = tempfile::tempdir().expect("temp root");
        let entry = root.path().join("extensions/foo/bar/src/index.ts");
        std::fs::create_dir_all(entry.parent().expect("parent")).expect("mkdir");
        std::fs::write(&entry, "export {}").expect("write");
        std::fs::create_dir_all(root.path().join("extensions/bar")).expect("mkdir sibling");

        let (ext_dir, source) =
            resolve_extension_paths(root.path(), "Foo/Bar", "src/index.ts", "Nested").expect("ok");
        assert!(source.is_file());
        assert_eq!(
            std::fs::canonicalize(&ext_dir).expect("ext dir"),
            std::fs::canonicalize(root.path().join("extensions/foo/bar")).expect("nested"),
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolve_extension_paths_rejects_source_symlink_escaping_extension_dir() {
        let root = tempfile::tempdir().expect("temp root");
        let ext_src = root.path().join("extensions/widget/src");
        std::fs::create_dir_all(&ext_src).expect("mkdir");
        let secret = root.path().join("secret.env");
        std::fs::write(&secret, "password").expect("write secret");
        std::os::unix::fs::symlink(&secret, ext_src.join("index.ts")).expect("symlink");

        let err = resolve_extension_paths(root.path(), "widget", "src/index.ts", "Widget")
            .expect_err("symlink escape");
        assert!(
            err.contains("Invalid extension source path for 'Widget'"),
            "unexpected: {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolve_extension_paths_rejects_extension_dir_symlink_escaping_extensions() {
        let root = tempfile::tempdir().expect("temp root");
        std::fs::create_dir_all(root.path().join("extensions")).expect("mkdir");
        let outside = root.path().join("outside");
        std::fs::create_dir_all(&outside).expect("mkdir outside");
        std::os::unix::fs::symlink(&outside, root.path().join("extensions/widget"))
            .expect("symlink");

        let err = resolve_extension_paths(root.path(), "widget", "src/index.ts", "Widget")
            .expect_err("dir symlink escape");
        assert!(
            err.contains("Invalid extension handle path"),
            "unexpected: {err}"
        );
    }

    /// On case-sensitive volumes, a symlink into a differently cased sibling
    /// (`EXTENSIONS` vs `extensions`) must not pass the realpath sandbox.
    #[cfg(unix)]
    #[test]
    fn enforce_sandbox_realpath_rejects_differently_cased_sibling_after_canonicalize() {
        let root = tempfile::tempdir().expect("temp root");
        let lower = root.path().join("extensions");
        let upper = root.path().join("EXTENSIONS");
        std::fs::create_dir_all(&lower).expect("mkdir lower");
        if std::fs::create_dir(&upper).is_err() {
            // Case-insensitive volume — both names are the same directory.
            return;
        }
        let lower_canon = std::fs::canonicalize(&lower).expect("canon lower");
        let upper_canon = std::fs::canonicalize(&upper).expect("canon upper");
        if lower_canon == upper_canon {
            return;
        }

        std::fs::write(upper.join("secret.ts"), "leak").expect("write");
        let link = lower.join("widget");
        std::os::unix::fs::symlink(&upper, &link).expect("symlink");

        let err = enforce_sandbox_realpath(&lower, &link, || "escaped".into())
            .expect_err("case-variant sibling must not count as inside");
        assert_eq!(err, "escaped");
    }

    #[test]
    fn normalize_extension_source_strips_to_handle_relative() {
        let root = tempfile::tempdir().expect("temp root");
        let entry = root.path().join("extensions/widget/src/index.ts");
        std::fs::create_dir_all(entry.parent().expect("parent")).expect("mkdir");
        std::fs::write(&entry, "export {}").expect("write");

        let from_handle_rel =
            normalize_extension_source_for_config(root.path(), "widget", "src/index.ts", "Widget")
                .expect("ok");
        assert_eq!(from_handle_rel, "src/index.ts");

        let from_repo_rel = normalize_extension_source_for_config(
            root.path(),
            "widget",
            "extensions/widget/src/index.ts",
            "Widget",
        )
        .expect("ok");
        assert_eq!(from_repo_rel, "src/index.ts");
    }

    #[test]
    fn normalize_extension_source_requires_existing_file() {
        let root = tempfile::tempdir().expect("temp root");
        let err = normalize_extension_source_for_config(
            root.path(),
            "widget",
            "src/missing.ts",
            "Widget",
        )
        .expect_err("missing file");
        assert!(
            err.contains("Extension source file not found for 'Widget'"),
            "unexpected: {err}"
        );
    }
}
