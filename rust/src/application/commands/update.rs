//! `gddy platform app update` — update label, description, or status.

use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use serde_json::json;

use super::schemas::ApplicationUpdate;
use crate::next_action::{next_action, required_value};
use crate::scopes::{APP_REGISTRY_READ, APP_REGISTRY_WRITE};

/// "At least one of" fields for `update`, flattened into [`UpdateArgs`].
///
/// Kept as its own derive struct (rather than inline fields on `UpdateArgs`)
/// so the struct-level `#[group(...)]` only covers these three — `id` stays
/// outside the group and independently required. See the flatten caveat on
/// `CommandSpec::from_args`: a struct can't both flatten a field and declare
/// its own enforced group.
#[derive(Debug, Clone, clap::Args)]
#[group(required = true, multiple = true)]
struct UpdateFields {
    /// New label.
    #[arg(long, value_name = "LABEL")]
    label: Option<String>,

    /// New description.
    #[arg(long, value_name = "TEXT")]
    description: Option<String>,

    /// Application status (ACTIVE or INACTIVE).
    #[arg(long, value_name = "STATUS", value_parser = ["ACTIVE", "INACTIVE"])]
    status: Option<String>,
}

#[derive(Debug, Clone, clap::Args)]
struct UpdateArgs {
    /// Application ID.
    #[arg(long, value_name = "ID")]
    id: String,

    #[command(flatten)]
    fields: UpdateFields,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<UpdateArgs, _, _, _>(
        CommandSpec::from_args::<UpdateArgs>("update", "Update an application")
            .with_long(
                "Update the label, description, or status of a GoDaddy \
                developer-platform application by its ID. At least one of \
                --label, --description, or --status must be provided. Use \
                `gddy platform app info --name <name>` to retrieve the \
                application ID.",
            )
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .with_scopes(&[APP_REGISTRY_READ, APP_REGISTRY_WRITE])
            .with_output_schema::<ApplicationUpdate>(),
        |context, args: UpdateArgs| async move {
            let mut input = serde_json::Map::new();
            if let Some(label) = args.fields.label {
                input.insert("label".to_owned(), json!(label));
            }
            if let Some(description) = args.fields.description {
                input.insert("description".to_owned(), json!(description));
            }
            if let Some(status) = args.fields.status {
                input.insert("status".to_owned(), json!(status));
            }
            let client = super::make_client(&context).await?;
            let data = client
                .update_application(&args.id, json!(input))
                .await
                .map_err(super::client_err)?;
            let name = data["updateApplication"]["name"]
                .as_str()
                .unwrap_or("")
                .to_owned();
            Ok(
                CommandResult::new(data["updateApplication"].clone()).with_next_actions(vec![
                    next_action(
                        "platform app info --name <name>",
                        "Inspect updated application",
                    )
                    .with_param("name", required_value(&name)),
                    next_action(
                        "platform app deploy --name <name>",
                        "Deploy updated application",
                    )
                    .with_param("name", required_value(&name)),
                ]),
            )
        },
    )
}

#[cfg(test)]
mod tests {
    fn update_clap_command() -> clap::Command {
        super::command().spec.clap_command()
    }

    #[test]
    fn status_rejects_values_outside_active_inactive() {
        for bad in ["active", "DISABLED", "PENDING"] {
            let err = update_clap_command()
                .try_get_matches_from(["update", "--id", "app-1", "--status", bad])
                .expect_err("invalid --status should be rejected");
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::InvalidValue,
                "--status {bad:?} should fail possible-value validation, got: {err}"
            );
        }
    }

    #[test]
    fn status_accepts_active_and_inactive() {
        for good in ["ACTIVE", "INACTIVE"] {
            update_clap_command()
                .try_get_matches_from(["update", "--id", "app-1", "--status", good])
                .expect("ACTIVE|INACTIVE --status should be accepted");
        }
    }

    #[test]
    fn update_requires_at_least_one_field() {
        let err = update_clap_command()
            .try_get_matches_from(["update", "--id", "app-1"])
            .expect_err("update with only --id should be rejected at parse time");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::MissingRequiredArgument,
            "expected MissingRequiredArgument, got: {err}"
        );
    }

    #[test]
    fn update_accepts_any_single_field() {
        update_clap_command()
            .try_get_matches_from(["update", "--id", "app-1", "--label", "New"])
            .expect("--label alone should be accepted");
        update_clap_command()
            .try_get_matches_from(["update", "--id", "app-1", "--description", "Desc"])
            .expect("--description alone should be accepted");
        update_clap_command()
            .try_get_matches_from(["update", "--id", "app-1", "--status", "ACTIVE"])
            .expect("--status alone should be accepted");
    }

    #[test]
    fn update_accepts_multiple_fields() {
        update_clap_command()
            .try_get_matches_from([
                "update",
                "--id",
                "app-1",
                "--label",
                "New",
                "--description",
                "Desc",
                "--status",
                "INACTIVE",
            ])
            .expect("multiple update fields should be allowed together");
    }
}
