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
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let path_str = path.to_string_lossy();
        if should_exclude(&path_str, excludes) {
            continue;
        }
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
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
