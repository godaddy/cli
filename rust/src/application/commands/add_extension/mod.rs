//! `gddy platform app add extension` — register embed/checkout/blocks UI
//! extensions into godaddy.toml. Nested under [`super::add`]'s `add` group.

use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};
use serde_json::json;

use super::schemas::{ExtensionBlocks, ExtensionHandle};

/// Shared shape for `add extension embed`/`add extension checkout` — both
/// register a UI extension by name/handle/source/target.
#[derive(Debug, Clone, clap::Args)]
struct UiExtensionArgs {
    /// Unique extension name written into godaddy.toml.
    #[arg(long)]
    name: String,

    /// Platform handle used to identify this extension surface.
    #[arg(long)]
    handle: String,

    /// Path to the JavaScript entry-point file, relative to
    /// extensions/<handle>/ (e.g. src/index.ts), or a path already under
    /// that directory.
    #[arg(long)]
    source: String,

    /// Comma-separated target surface(s) for this extension.
    #[arg(long)]
    target: String,
}

#[derive(Debug, Clone, clap::Args)]
struct BlocksArgs {
    /// Path to the JavaScript entry-point file for the blocks extension,
    /// relative to extensions/blocks/ (e.g. src/index.ts), or a path
    /// already under that directory.
    #[arg(long)]
    source: String,
}

/// Parse a comma-separated `--target` value into extension targets, trimming
/// whitespace and dropping empty entries.
fn parse_targets(raw: &str) -> Vec<crate::config::ExtensionTarget> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| crate::config::ExtensionTarget {
            target: s.to_owned(),
        })
        .collect()
}

/// Sandbox + normalize `--source` the same way deploy resolves it, so
/// `godaddy.toml` always stores a path relative to `extensions/{handle}/`.
fn normalized_extension_source(
    handle: &str,
    source: &str,
    extension_name: &str,
) -> cli_engine::Result<String> {
    let repo_root = crate::extension::repo_root_from_cwd();
    crate::extension::normalize_extension_source_for_config(
        &repo_root,
        handle,
        source,
        extension_name,
    )
    .map_err(super::validation_err)
}

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("extension", "Add an extension to godaddy.toml").with_long(
            "Append a UI extension entry to the godaddy.toml manifest in the \
            current directory. Extensions are JavaScript bundles that the \
            platform renders inside store surfaces. Three types are supported: \
            embed (inline widget), checkout (checkout-flow widget), and blocks \
            (content blocks). Run `gddy platform app deploy` to bundle and \
            upload the registered extensions.",
        ),
    )
    .with_command(RuntimeCommandSpec::new_typed_with_context::<
        UiExtensionArgs,
        _,
        _,
        _,
    >(
        CommandSpec::from_args::<UiExtensionArgs>("embed", "Add an embed extension")
            .with_long(
                "Register an embed UI extension in godaddy.toml. An embed \
                extension is a JavaScript bundle rendered as an inline widget \
                inside a store surface. Provide a unique name, a platform \
                handle, and the path to the source entry-point file. Run \
                `gddy platform app deploy` to bundle and upload.",
            )
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .with_output_schema::<ExtensionHandle>()
            .no_auth(true),
        |ctx, args: UiExtensionArgs| async move {
            let name = args.name;
            let handle = args.handle;
            let source = normalized_extension_source(&handle, &args.source, &name)?;
            let targets = parse_targets(&args.target);
            if targets.is_empty() {
                return Err(crate::error::GddyError::validation(
                    "at least one --target is required (comma-separated)",
                )
                .into_cli_error());
            }
            let path = crate::config::config_path(Some(&ctx.middleware.env));
            let mut config = crate::config::read_config(&path)
                .map_err(|e| crate::error::GddyError::config(e.to_string()).into_cli_error())?;
            let exts = config
                .extensions
                .get_or_insert_with(|| crate::config::ExtensionsConfig {
                    embed: vec![],
                    checkout: vec![],
                    blocks: None,
                });
            exts.embed.push(crate::config::EmbedExtensionConfig {
                name: name.clone(),
                handle: handle.clone(),
                source,
                targets,
            });
            crate::config::write_config(&path, &config)
                .map_err(|e| crate::error::GddyError::config(e.to_string()).into_cli_error())?;
            Ok(
                CommandResult::new(json!({ "name": name, "handle": handle, "type": "embed" }))
                    .with_next_actions(super::add_config_next_actions(&config.name)),
            )
        },
    ))
    .with_command(RuntimeCommandSpec::new_typed_with_context::<
        UiExtensionArgs,
        _,
        _,
        _,
    >(
        CommandSpec::from_args::<UiExtensionArgs>("checkout", "Add a checkout extension")
            .with_long(
                "Register a checkout UI extension in godaddy.toml. A checkout \
                extension is a JavaScript bundle rendered during the store \
                checkout flow. Provide a unique name, a platform handle, and the \
                path to the source entry-point file. Run \
                `gddy platform app deploy` to bundle and upload.",
            )
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .with_output_schema::<ExtensionHandle>()
            .no_auth(true),
        |ctx, args: UiExtensionArgs| async move {
            let name = args.name;
            let handle = args.handle;
            let source = normalized_extension_source(&handle, &args.source, &name)?;
            let targets = parse_targets(&args.target);
            if targets.is_empty() {
                return Err(crate::error::GddyError::validation(
                    "at least one --target is required (comma-separated)",
                )
                .into_cli_error());
            }
            let path = crate::config::config_path(Some(&ctx.middleware.env));
            let mut config = crate::config::read_config(&path)
                .map_err(|e| crate::error::GddyError::config(e.to_string()).into_cli_error())?;
            let exts = config
                .extensions
                .get_or_insert_with(|| crate::config::ExtensionsConfig {
                    embed: vec![],
                    checkout: vec![],
                    blocks: None,
                });
            exts.checkout.push(crate::config::CheckoutExtensionConfig {
                name: name.clone(),
                handle: handle.clone(),
                source,
                targets,
            });
            crate::config::write_config(&path, &config)
                .map_err(|e| crate::error::GddyError::config(e.to_string()).into_cli_error())?;
            Ok(
                CommandResult::new(json!({ "name": name, "handle": handle, "type": "checkout" }))
                    .with_next_actions(super::add_config_next_actions(&config.name)),
            )
        },
    ))
    .with_command(RuntimeCommandSpec::new_typed_with_context::<
        BlocksArgs,
        _,
        _,
        _,
    >(
        CommandSpec::from_args::<BlocksArgs>("blocks", "Add a blocks extension")
            .with_long(
                "Register a blocks UI extension in godaddy.toml. A blocks \
                extension is a JavaScript bundle that provides content blocks \
                within a store. Only one blocks extension is supported per \
                application; running this command again will overwrite the \
                existing entry. Run `gddy platform app deploy` to bundle and \
                upload.",
            )
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .with_output_schema::<ExtensionBlocks>()
            .no_auth(true),
        |ctx, args: BlocksArgs| async move {
            let source = normalized_extension_source("blocks", &args.source, "Blocks")?;
            let path = crate::config::config_path(Some(&ctx.middleware.env));
            let mut config = crate::config::read_config(&path)
                .map_err(|e| crate::error::GddyError::config(e.to_string()).into_cli_error())?;
            let exts = config
                .extensions
                .get_or_insert_with(|| crate::config::ExtensionsConfig {
                    embed: vec![],
                    checkout: vec![],
                    blocks: None,
                });
            exts.blocks = Some(crate::config::BlocksExtensionConfig {
                source: source.clone(),
            });
            crate::config::write_config(&path, &config)
                .map_err(|e| crate::error::GddyError::config(e.to_string()).into_cli_error())?;
            Ok(
                CommandResult::new(json!({ "source": source, "type": "blocks" }))
                    .with_next_actions(super::add_config_next_actions(&config.name)),
            )
        },
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_targets_trims_splits_and_drops_empties() {
        let parsed = super::parse_targets(" a , b ,, c ");
        let got: Vec<String> = parsed.into_iter().map(|t| t.target).collect();
        assert_eq!(got, vec!["a", "b", "c"]);
        assert!(super::parse_targets("   ").is_empty());
        assert!(super::parse_targets("").is_empty());
    }
}
