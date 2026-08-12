//! Discover source files under an extension package directory.

use std::path::{Path, PathBuf};

use globset::GlobSet;

use super::config::{exclude_matcher, security_config, should_exclude};

const SOURCE_EXTENSIONS: &[&str] = &[".js", ".ts", ".jsx", ".tsx", ".mjs", ".cjs"];

/// Recursively collect source files under `root`, applying exclude globs.
pub fn find_files_to_scan(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let config = security_config();
    let excludes = exclude_matcher(&config);
    let mut files = Vec::new();
    if !root.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("path is not a directory: {}", root.display()),
        ));
    }
    traverse(root, &excludes, &mut files)?;
    Ok(files)
}

fn traverse(dir: &Path, excludes: &GlobSet, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let dir_str = dir.to_string_lossy();
    if should_exclude(&dir_str, excludes) {
        return Ok(());
    }
    let entries = std::fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let path_str = path.to_string_lossy();
        if should_exclude(&path_str, excludes) {
            continue;
        }
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            return Err(std::io::Error::other(format!(
                "symlink encountered during security scan: {}",
                path.display()
            )));
        }
        if ft.is_dir() {
            traverse(&path, excludes, out)?;
        } else if ft.is_file() {
            // Case-sensitive
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if SOURCE_EXTENSIONS.iter().any(|ext| name.ends_with(ext)) {
                out.push(path);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn unreadable_subdirectory_fails_closed() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).expect("mkdir");
        std::fs::write(dir.path().join("ok.ts"), "export {};\n").expect("write");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("chmod");

        let result = find_files_to_scan(dir.path());

        // Restore so tempfile cleanup succeeds.
        let _ = std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755));
        assert!(
            result.is_err(),
            "expected discovery to fail closed on unreadable dir, got {result:?}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn symlink_fails_closed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("target.ts");
        std::fs::write(&target, "eval('x');\n").expect("write");
        std::os::unix::fs::symlink(&target, dir.path().join("link.ts")).expect("symlink");

        let err = find_files_to_scan(dir.path()).expect_err("symlink should fail closed");
        assert!(err.to_string().contains("symlink"), "unexpected err: {err}");
    }
}
