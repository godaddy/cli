use std::path::{Path, PathBuf};

use super::sandbox::repo_root_from_cwd;
use super::types::ExtensionType;

// ---------------------------------------------------------------------------
// UI extension runtime wrapper generation
// ---------------------------------------------------------------------------

/// Absolute path suitable for embedding in the UI runtime wrapper import.
/// Relative sources must not be resolved against the temp wrapper location.
pub(super) async fn absolute_entry_path(source_path: &Path) -> PathBuf {
    if source_path.is_absolute() {
        return tokio::fs::canonicalize(source_path)
            .await
            .unwrap_or_else(|_| source_path.to_path_buf());
    }
    let joined = repo_root_from_cwd().join(source_path);
    tokio::fs::canonicalize(&joined).await.unwrap_or(joined)
}

/// True for embed/checkout — they need the UI runtime registration wrapper.
pub(super) fn should_use_ui_extension_runtime_wrapper(ext_type: ExtensionType) -> bool {
    matches!(ext_type, ExtensionType::Embed | ExtensionType::Checkout)
}

/// Synthetic entry that imports the user module and registers it with
/// `globalThis.GoDaddyUiExtensions`.
///
/// `entry_path` must be absolute so esbuild resolves it from the temp wrapper
/// file location.
pub(super) fn create_ui_extension_runtime_wrapper(entry_path: &Path) -> String {
    let entry = entry_path.to_string_lossy();
    format!(
        r#"import * as userModule from {entry:?};

function resolveContract() {{
  if (typeof userModule.mount === "function") {{
    return userModule;
  }}

  const defaultExport = userModule.default;
  const candidate = typeof defaultExport === "function"
    ? defaultExport()
    : defaultExport;

  if (!candidate || typeof candidate.mount !== "function") {{
    throw new Error("UI extension must export mount or a default contract/factory.");
  }}

  return candidate;
}}

const contract = resolveContract();
const registry = globalThis.GoDaddyUiExtensions;

if (!registry || typeof registry.register !== "function") {{
  throw new Error("UI extension runtime registry is not available.");
}}

registry.register(contract);
"#
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ui_wrapper_embeds_absolute_entry_path() {
        let root = tempfile::tempdir().expect("temp");
        let entry = root.path().join("src").join("index.ts");
        std::fs::create_dir_all(entry.parent().expect("parent")).expect("mkdir");
        std::fs::write(&entry, "export function mount() {}").expect("write");
        let abs = absolute_entry_path(&entry).await;
        assert!(abs.is_absolute(), "{abs:?}");
        let wrapper = create_ui_extension_runtime_wrapper(&abs);
        assert!(
            wrapper.contains(&abs.to_string_lossy().replace('\\', "\\\\"))
                || wrapper.contains(abs.to_string_lossy().as_ref()),
            "wrapper should import absolute path: {wrapper}"
        );
    }

    #[test]
    fn ui_runtime_wrapper_only_for_embed_and_checkout() {
        assert!(should_use_ui_extension_runtime_wrapper(
            ExtensionType::Embed
        ));
        assert!(should_use_ui_extension_runtime_wrapper(
            ExtensionType::Checkout
        ));
        assert!(!should_use_ui_extension_runtime_wrapper(
            ExtensionType::Blocks
        ));
    }

    #[test]
    fn ui_runtime_wrapper_registers_contract() {
        let wrapper =
            create_ui_extension_runtime_wrapper(Path::new("/path/to/extension/src/index.ts"));
        assert!(wrapper.contains("import * as userModule from"));
        assert!(wrapper.contains("registry.register(contract)"));
        assert!(wrapper.contains("GoDaddyUiExtensions"));
        assert!(wrapper.contains(r#"typeof candidate.mount !== "function""#));
    }
}
