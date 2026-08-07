use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionType {
    Embed,
    Checkout,
    Blocks,
}

/// Options for [`crate::extension::bundle_extension`].
pub struct BundleOptions<'a> {
    /// Extension handle — used as the artifact package name.
    pub name: &'a str,
    /// Optional semver; defaults to `"0.0.0"` in the artifact filename.
    pub version: Option<&'a str>,
    /// Repo root (for temp-dir naming and root `tsconfig.json` fallback).
    pub repo_root: &'a Path,
    /// UTC timestamp override (`yyyymmddHHMMss`); defaults to now.
    pub timestamp: Option<&'a str>,
}

/// Result of a successful bundle. Artifacts remain on disk under [`Self::temp_dir`]
/// until [`BundleCleanup`] removes them.
pub struct BundleResult {
    pub bytes: Vec<u8>,
    pub sha256: String,
    pub artifact_name: String,
    pub artifact_path: PathBuf,
    pub size: u64,
    pub sourcemap_path: Option<PathBuf>,
    /// Root temp directory created for this bundle; pass to cleanup.
    pub temp_dir: PathBuf,
}

/// Best-effort RAII cleanup of a bundle temp directory.
///
/// Construct with [`Self::for_dir`] as soon as the temp dir exists (so early
/// failures still clean up), then [`Self::disarm`] before returning a successful
/// [`BundleResult`] so the caller can own cleanup via [`Self::new`].
pub struct BundleCleanup {
    temp_dir: Option<PathBuf>,
}

impl BundleCleanup {
    pub fn new(bundle: &BundleResult) -> Self {
        Self::for_dir(bundle.temp_dir.clone())
    }

    pub fn for_dir(temp_dir: PathBuf) -> Self {
        Self {
            temp_dir: Some(temp_dir),
        }
    }

    /// Leave the directory in place (caller takes ownership of cleanup).
    pub fn disarm(mut self) {
        self.temp_dir = None;
    }
}

impl Drop for BundleCleanup {
    fn drop(&mut self) {
        if let Some(dir) = self.temp_dir.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
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
