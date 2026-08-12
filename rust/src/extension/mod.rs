mod bundler;
mod runtime_wrapper;
mod sandbox;
mod security;
mod types;

pub use bundler::bundle_extension;
pub(crate) use bundler::format_timestamp;
pub(crate) use sandbox::{
    normalize_extension_source_for_config, repo_root_from_cwd, require_extension_source_file,
    resolve_extension_paths,
};
pub use security::{is_blocked, scan_bundle, scan_extension};
pub use types::{BundleCleanup, BundleOptions, ExtensionType, Severity};
// Public scan/bundle surface types; no in-crate caller names them via
// `crate::extension::…` yet, so the plain re-export would be flagged unused.
#[allow(unused_imports)]
pub use security::{ScanError, ScanReport};
#[allow(unused_imports)]
pub use types::{BundleResult, Finding};
