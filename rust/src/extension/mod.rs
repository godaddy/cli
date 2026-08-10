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
pub use security::{is_blocked, scan_bundle};
pub use types::{BundleCleanup, BundleOptions, ExtensionType, Severity};
// `BundleResult` and `Finding` are part of the module's public surface (returned
// from `bundle_extension` / `scan_bundle`) but no caller currently names them via
// `crate::extension::…`, so the plain re-export would be flagged as unused.
#[allow(unused_imports)]
pub use types::{BundleResult, Finding};
