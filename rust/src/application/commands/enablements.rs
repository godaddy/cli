//! `gddy platform app enablements` — list apps enabled on a commerce store.

use cli_engine::{
    CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, TableColumn, Tier,
};
use serde_json::{Value, json};

use super::schemas::StoreEnablement;
use crate::next_action::{next_action, required_value};
use crate::scopes::APP_REGISTRY_READ;

#[derive(Debug, Clone, clap::Args)]
struct EnablementsArgs {
    /// Commerce store ID whose enablements to list.
    #[arg(long = "store-id", value_name = "STORE_ID")]
    store_id: String,
}

fn view_columns() -> Vec<TableColumn> {
    vec![
        TableColumn::new("name", "Name"),
        TableColumn::new("status", "Status"),
        TableColumn::new("releaseVersion", "Release"),
        TableColumn::new("label", "Label"),
        TableColumn::new("id", "ID"),
    ]
}

/// Flatten App Registry enablement rows for CLI output.
///
/// Lifts `release.version` to top-level `releaseVersion` so defaults can show
/// the version string without embedding the whole `release` object (dotted
/// `--fields` paths like `release.version` are not supported).
fn flatten_enablements(data: Value) -> Value {
    let Some(rows) = data.as_array() else {
        return json!([]);
    };
    let flattened: Vec<Value> = rows
        .iter()
        .map(|row| {
            let release_version = row
                .pointer("/release/version")
                .cloned()
                .unwrap_or(Value::Null);
            json!({
                "id": row.get("id").cloned().unwrap_or(Value::Null),
                "name": row.get("name").cloned().unwrap_or(Value::Null),
                "label": row.get("label").cloned().unwrap_or(Value::Null),
                "status": row.get("status").cloned().unwrap_or(Value::Null),
                "releaseVersion": release_version,
            })
        })
        .collect();
    json!(flattened)
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<EnablementsArgs, _, _, _>(
        CommandSpec::from_args::<EnablementsArgs>(
            "enablements",
            "List applications enabled on a store",
        )
        .with_long(
            "List GoDaddy developer-platform applications currently enabled on \
            a specific commerce store. This is the read counterpart to \
            `gddy platform app enable` / `disable`. Requires a store ID; returns \
            an empty list when nothing is enabled. Application status is the \
            registry lifecycle (usually ACTIVE), not the enablement itself — \
            presence in this list means the app is enabled on the store. Defaults \
            show name, status, and releaseVersion; pass `--fields id,label,...` \
            (or `--fields all`) to include the application id and/or label.",
        )
        .with_system("applications")
        .with_tier(Tier::Read)
        .with_scopes(&[APP_REGISTRY_READ])
        .with_default_fields("name,status,releaseVersion")
        .with_output_schema::<StoreEnablement>()
        .with_view(view_columns()),
        |ctx, args: EnablementsArgs| async move {
            let store_id = args.store_id;
            let client = super::make_client(&ctx).await?;
            let data = client
                .list_enabled_store_applications(&store_id)
                .await
                .map_err(super::client_err)?;
            Ok(
                CommandResult::new(flatten_enablements(data)).with_next_actions(vec![
                    next_action(
                        "platform app enable <name> --store-id <store-id>",
                        "Enable an application on this store",
                    )
                    .with_param("name", NextActionParam::required())
                    .with_param("store-id", required_value(&store_id)),
                    next_action(
                        "platform app disable <name> --store-id <store-id>",
                        "Disable an application on this store",
                    )
                    .with_param("name", NextActionParam::required())
                    .with_param("store-id", required_value(&store_id)),
                    next_action(
                        "platform app info --name <name>",
                        "Inspect a listed application",
                    )
                    .with_param("name", NextActionParam::required()),
                ]),
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig, Stage};
    use serde_json::json;

    use super::{command, flatten_enablements};

    #[test]
    fn command_requires_store_id_flag() {
        command()
            .spec
            .clap_command()
            .try_get_matches_from(["enablements"])
            .expect_err("--store-id is required");
    }

    #[test]
    fn command_accepts_store_id_flag() {
        command()
            .spec
            .clap_command()
            .try_get_matches_from(["enablements", "--store-id", "store-123"])
            .expect("--store-id should be accepted");
    }

    #[test]
    fn default_fields_are_name_status_release_version() {
        assert_eq!(
            command().spec.default_fields.as_deref(),
            Some("name,status,releaseVersion")
        );
    }

    #[test]
    fn flatten_enablements_lifts_release_version_only() {
        let input = json!([{
            "id": "app-1",
            "name": "my-app",
            "label": "My App",
            "status": "ACTIVE",
            "release": { "id": "rel-1", "version": "1.2.3" }
        }]);
        assert_eq!(
            flatten_enablements(input),
            json!([{
                "id": "app-1",
                "name": "my-app",
                "label": "My App",
                "status": "ACTIVE",
                "releaseVersion": "1.2.3"
            }])
        );
    }

    #[test]
    fn flatten_enablements_null_release_version_when_missing() {
        let input = json!([{ "id": "app-1", "name": "my-app", "status": "ACTIVE" }]);
        assert_eq!(
            flatten_enablements(input)[0]["releaseVersion"],
            json!(null)
        );
    }

    #[test]
    fn flatten_enablements_null_label_when_missing() {
        let input = json!([{ "id": "app-1", "name": "my-app", "status": "ACTIVE" }]);
        assert_eq!(flatten_enablements(input)[0]["label"], json!(null));
    }

    #[tokio::test]
    async fn platform_app_enablements_requires_auth() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_min_stage(Stage::Experimental)
                .with_default_auth_provider("godaddy")
                .with_module(crate::platform::module()),
        );

        let output = cli
            .run([
                "gddy",
                "platform",
                "app",
                "enablements",
                "--store-id",
                "store-123",
                "--output",
                "json",
            ])
            .await;

        const AUTH_FAILURE_EXIT: i32 = 2;
        assert_eq!(
            output.exit_code, AUTH_FAILURE_EXIT,
            "platform app enablements must fail closed at auth resolution, got: {}",
            output.rendered
        );
        let json: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json output");
        let message = json["error"]["message"].as_str().unwrap_or_default();
        assert!(
            message.contains("provider"),
            "expected an auth-provider resolution error, got: {}",
            output.rendered
        );
    }
}
