//! Extension collection, bundling, security scanning, and upload logic used
//! by [`super::command`]'s deploy orchestration.

use cli_engine::StreamSender;
use serde_json::{Value, json};

use crate::application::client::{ApplicationClient, UploadOptions};

/// One extension entry from godaddy.toml ready for deploy: name, handle,
/// source, type, and target surfaces (empty for blocks / untargeted).
pub(super) struct ExtensionDeploy {
    name: String,
    handle: String,
    source: String,
    ext_type: crate::extension::ExtensionType,
    targets: Vec<String>,
}

pub(super) fn collect_extensions(config: &crate::config::Config) -> Vec<ExtensionDeploy> {
    let mut result = Vec::new();
    if let Some(exts) = &config.extensions {
        for e in &exts.embed {
            result.push(ExtensionDeploy {
                name: e.name.clone(),
                handle: e.handle.clone(),
                source: e.source.clone(),
                ext_type: crate::extension::ExtensionType::Embed,
                targets: e.targets.iter().map(|t| t.target.clone()).collect(),
            });
        }
        for e in &exts.checkout {
            result.push(ExtensionDeploy {
                name: e.name.clone(),
                handle: e.handle.clone(),
                source: e.source.clone(),
                ext_type: crate::extension::ExtensionType::Checkout,
                targets: e.targets.iter().map(|t| t.target.clone()).collect(),
            });
        }
        if let Some(blocks) = &exts.blocks {
            result.push(ExtensionDeploy {
                name: "Blocks".to_owned(),
                handle: "blocks".to_owned(),
                source: blocks.source.clone(),
                ext_type: crate::extension::ExtensionType::Blocks,
                targets: Vec::new(),
            });
        }
    }
    result
}

/// Resolve the upload target(s) for one extension. A blocks extension always
/// uploads to the `blocks` target; embed/checkout upload once per configured
/// target, or a single untargeted upload (`None`) when none are configured.
fn resolve_upload_targets(
    ext_type: crate::extension::ExtensionType,
    targets: &[String],
) -> Vec<Option<String>> {
    match ext_type {
        crate::extension::ExtensionType::Blocks => vec![Some("blocks".to_owned())],
        _ if targets.is_empty() => vec![None],
        _ => targets.iter().cloned().map(Some).collect(),
    }
}

fn upload_completed_event(
    extension_name: &str,
    target: &Option<String>,
    is_final_target: bool,
    index: usize,
    total: usize,
) -> Value {
    let mut event = json!({
        "type": "progress",
        "name": "extension.upload",
        "status": "completed",
        "extensionName": extension_name,
        "target": target,
    });
    if is_final_target {
        event["percent"] = json!(index * 100 / total.max(1));
    }
    event
}

pub(super) struct DeployExtensionArgs<'a> {
    pub(super) application_id: &'a str,
    pub(super) release_id: &'a str,
    pub(super) ext: &'a ExtensionDeploy,
    pub(super) index: usize,
    pub(super) total: usize,
    pub(super) deploy_timestamp: &'a str,
}

pub(super) async fn deploy_extension(
    client: &ApplicationClient,
    sender: &StreamSender,
    args: DeployExtensionArgs<'_>,
) -> cli_engine::Result<()> {
    let DeployExtensionArgs {
        application_id,
        release_id,
        ext,
        index,
        total,
        deploy_timestamp,
    } = args;
    let ext_name = &ext.name;
    let ext_type = ext.ext_type;

    // ---- Bundle ----
    sender
        .send(json!({
            "type": "progress",
            "name": "extension.bundle",
            "status": "started",
            "extensionName": ext_name,
            "percent": ((index - 1) * 100 / total.max(1)),
        }))
        .await;

    let repo_root = crate::extension::repo_root_from_cwd();
    let (ext_dir, source) =
        crate::extension::resolve_extension_paths(&repo_root, &ext.handle, &ext.source, ext_name)
            .map_err(super::super::validation_err)?;
    crate::extension::require_extension_source_file(ext.handle.as_str(), ext_name, &source)
        .map_err(super::super::validation_err)?;
    let bundle = crate::extension::bundle_extension(
        &source,
        ext_type,
        &ext_dir,
        crate::extension::BundleOptions {
            name: &ext.handle,
            version: None,
            repo_root: &repo_root,
            timestamp: Some(deploy_timestamp),
        },
    )
    .await
    .map_err(|e| super::super::validation_err(format!("bundle failed for '{ext_name}': {e}")))?;
    // Always clean temp artifacts when this function returns (success or error).
    let _bundle_cleanup = crate::extension::BundleCleanup::new(&bundle);

    sender
        .send(json!({
            "type": "progress",
            "name": "extension.bundle",
            "status": "completed",
            "extensionName": ext_name,
            "artifactName": bundle.artifact_name,
            "artifactPath": bundle.artifact_path.display().to_string(),
            "size": bundle.size,
            "sha256": bundle.sha256,
            "sourcemapPath": bundle.sourcemap_path.as_ref().map(|p| p.display().to_string()),
        }))
        .await;

    // ---- Security scan ----
    sender
        .send(json!({
            "type": "progress",
            "name": "extension.scan",
            "status": "started",
            "extensionName": ext_name,
            "artifactName": bundle.artifact_name,
        }))
        .await;

    let source_display = ext.source.as_str();
    let content = String::from_utf8_lossy(&bundle.bytes);
    let findings = crate::extension::scan_bundle(&content, source_display);

    if crate::extension::is_blocked(&findings) {
        let blocked_msgs: Vec<String> = findings
            .iter()
            .filter(|f| f.severity == crate::extension::Severity::Block)
            .map(|f| {
                if f.snippet.is_empty() {
                    format!("  {} ({}:{}): {}", f.rule_id, f.file, f.line, f.message)
                } else {
                    format!(
                        "  {} ({}:{}): {}\n    > {}",
                        f.rule_id, f.file, f.line, f.message, f.snippet
                    )
                }
            })
            .collect();
        return Err(crate::error::GddyError::security(format!(
            "security scan blocked deployment of '{ext_name}':\n{}",
            blocked_msgs.join("\n")
        ))
        .into_cli_error());
    }

    sender
        .send(json!({
            "type": "progress",
            "name": "extension.scan",
            "status": "completed",
            "extensionName": ext_name,
            "findings": findings.len(),
        }))
        .await;

    let bytes = bytes::Bytes::from(bundle.bytes);

    // Upload the bundle once per configured target (blocks -> "blocks";
    // embed/checkout -> each target, or a single untargeted upload).
    let upload_targets = resolve_upload_targets(ext_type, &ext.targets);
    for (target_index, target) in upload_targets.iter().enumerate() {
        sender
            .send(json!({ "type": "progress", "name": "extension.upload", "status": "started", "extensionName": ext_name, "target": target }))
            .await;

        let mut upload_input = json!({
            "applicationId": application_id,
            "releaseId": release_id,
            "contentType": "JS",
        });
        if let Some(t) = target {
            upload_input["target"] = json!(t);
        }

        let upload_data = client
            .generate_upload_url(upload_input)
            .await
            .map_err(super::super::client_err)?;

        let upload = &upload_data["generateReleaseUploadUrl"];
        let upload_url = upload["url"].as_str().unwrap_or("").to_owned();
        let upload_id = upload["uploadId"].as_str().unwrap_or("").to_owned();
        let max_size_bytes = upload["maxSizeBytes"].as_u64();

        // Parse required headers from ["key:value"] array
        let mut headers = serde_json::Map::new();
        if let Some(arr) = upload["requiredHeaders"].as_array() {
            for h in arr {
                if let Some(s) = h.as_str()
                    && let Some((k, v)) = s.split_once(':')
                {
                    headers.insert(k.trim().to_owned(), json!(v.trim()));
                }
            }
        }

        client
            .upload_artifact(
                &upload_url,
                &upload_id,
                &json!(headers),
                max_size_bytes,
                bytes.clone(),
                UploadOptions::default(),
            )
            .await
            .map_err(super::super::client_err)?;

        sender
            .send(upload_completed_event(
                ext_name,
                target,
                target_index + 1 == upload_targets.len(),
                index,
                total,
            ))
            .await;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::config::{
        BlocksExtensionConfig, CheckoutExtensionConfig, Config, EmbedExtensionConfig,
        ExtensionTarget, ExtensionsConfig,
    };

    /// Minimal `Config` with placeholder values for every field
    /// `collect_extensions` doesn't read — only `extensions` varies per test.
    fn base_config(extensions: Option<ExtensionsConfig>) -> Config {
        Config {
            name: "test-app".to_owned(),
            client_id: "00000000-0000-4000-8000-000000000000".to_owned(),
            description: None,
            version: "1.0.0".to_owned(),
            url: "https://example.com".to_owned(),
            proxy_url: "https://proxy.example.com".to_owned(),
            authorization_scopes: vec![],
            actions: vec![],
            subscriptions: None,
            dependencies: vec![],
            extensions,
        }
    }

    #[test]
    fn collect_extensions_returns_empty_when_none_configured() {
        assert!(super::collect_extensions(&base_config(None)).is_empty());
    }

    #[test]
    fn collect_extensions_maps_embed_extensions() {
        let config = base_config(Some(ExtensionsConfig {
            embed: vec![EmbedExtensionConfig {
                name: "@test/embed-one".to_owned(),
                handle: "embed-one".to_owned(),
                source: "src/index.ts".to_owned(),
                targets: vec![ExtensionTarget {
                    target: "admin.product.detail".to_owned(),
                }],
            }],
            checkout: vec![],
            blocks: None,
        }));

        let deploys = super::collect_extensions(&config);
        assert_eq!(deploys.len(), 1);
        assert_eq!(deploys[0].name, "@test/embed-one");
        assert_eq!(deploys[0].handle, "embed-one");
        assert_eq!(deploys[0].source, "src/index.ts");
        assert_eq!(deploys[0].ext_type, crate::extension::ExtensionType::Embed);
        assert_eq!(deploys[0].targets, vec!["admin.product.detail".to_owned()]);
    }

    #[test]
    fn collect_extensions_maps_checkout_extensions() {
        let config = base_config(Some(ExtensionsConfig {
            embed: vec![],
            checkout: vec![CheckoutExtensionConfig {
                name: "@test/checkout-one".to_owned(),
                handle: "checkout-one".to_owned(),
                source: "src/checkout.ts".to_owned(),
                targets: vec![],
            }],
            blocks: None,
        }));

        let deploys = super::collect_extensions(&config);
        assert_eq!(deploys.len(), 1);
        assert_eq!(deploys[0].name, "@test/checkout-one");
        assert_eq!(
            deploys[0].ext_type,
            crate::extension::ExtensionType::Checkout
        );
        assert!(deploys[0].targets.is_empty());
    }

    #[test]
    fn collect_extensions_maps_blocks_extension_with_fixed_name_and_handle() {
        let config = base_config(Some(ExtensionsConfig {
            embed: vec![],
            checkout: vec![],
            blocks: Some(BlocksExtensionConfig {
                source: "src/blocks.ts".to_owned(),
            }),
        }));

        let deploys = super::collect_extensions(&config);
        assert_eq!(deploys.len(), 1);
        // Blocks has no per-extension name/handle in godaddy.toml — these are
        // fixed, matching the single implicit "blocks" upload target.
        assert_eq!(deploys[0].name, "Blocks");
        assert_eq!(deploys[0].handle, "blocks");
        assert_eq!(deploys[0].source, "src/blocks.ts");
        assert_eq!(deploys[0].ext_type, crate::extension::ExtensionType::Blocks);
        assert!(deploys[0].targets.is_empty());
    }

    #[test]
    fn collect_extensions_preserves_embed_then_checkout_then_blocks_order() {
        let config = base_config(Some(ExtensionsConfig {
            embed: vec![EmbedExtensionConfig {
                name: "@test/embed".to_owned(),
                handle: "embed".to_owned(),
                source: "src/embed.ts".to_owned(),
                targets: vec![],
            }],
            checkout: vec![CheckoutExtensionConfig {
                name: "@test/checkout".to_owned(),
                handle: "checkout".to_owned(),
                source: "src/checkout.ts".to_owned(),
                targets: vec![],
            }],
            blocks: Some(BlocksExtensionConfig {
                source: "src/blocks.ts".to_owned(),
            }),
        }));

        let deploys = super::collect_extensions(&config);
        let names: Vec<&str> = deploys.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["@test/embed", "@test/checkout", "Blocks"]);
    }

    #[test]
    fn upload_completed_event_omits_percent_when_not_final_target() {
        let target = Some("admin.product.detail".to_owned());

        let event = super::upload_completed_event("widget", &target, false, 1, 2);

        assert_eq!(
            event,
            serde_json::json!({
                "type": "progress",
                "name": "extension.upload",
                "status": "completed",
                "extensionName": "widget",
                "target": "admin.product.detail",
            })
        );
    }

    #[test]
    fn resolve_upload_targets_by_type() {
        use crate::extension::ExtensionType;

        // Blocks always uploads to the fixed "blocks" target.
        assert_eq!(
            super::resolve_upload_targets(ExtensionType::Blocks, &[]),
            vec![Some("blocks".to_owned())]
        );
        // Embed/checkout with no targets: a single untargeted upload.
        assert_eq!(
            super::resolve_upload_targets(ExtensionType::Embed, &[]),
            vec![None]
        );
        // Embed/checkout with targets: one upload per target.
        assert_eq!(
            super::resolve_upload_targets(
                ExtensionType::Checkout,
                &["a".to_owned(), "b".to_owned()]
            ),
            vec![Some("a".to_owned()), Some("b".to_owned())]
        );
    }

    #[test]
    fn final_target_completion_preserves_legacy_upload_event() {
        let target = Some("admin.product.detail".to_owned());

        let event = super::upload_completed_event("widget", &target, true, 1, 1);

        assert_eq!(
            event,
            serde_json::json!({
                "type": "progress",
                "name": "extension.upload",
                "status": "completed",
                "extensionName": "widget",
                "target": "admin.product.detail",
                "percent": 100,
            })
        );
    }
}
