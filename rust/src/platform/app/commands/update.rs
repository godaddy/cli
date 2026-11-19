//! `gddy platform app update` — update label, description, distribution type,
//! and manifest OAuth redirects.

use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use platform_app_client::update_application::MutationUpdateApplicationInput;

use super::distribution_type::DistributionTypeArg;
use super::schemas::ApplicationUpdate;
use crate::next_action::{next_action, required_value};
use crate::scopes::{APP_REGISTRY_READ, APP_REGISTRY_WRITE};

/// "At least one of" fields for `update`, flattened into [`UpdateArgs`].
///
/// Kept as its own derive struct (rather than inline fields on `UpdateArgs`)
/// so the struct-level `#[group(...)]` only covers these fields — `id` stays
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

    /// Who can install the application: `public` (anyone, via the
    /// marketplace) or `private` (your organization only). Moving an
    /// application to private deactivates its active marketplace listing.
    #[arg(long, value_name = "TYPE", value_enum, ignore_case = true)]
    distribution_type: Option<DistributionTypeArg>,
}

/// `None` is omitted from the request (the `OmitNoneInInputs` pass in
/// `platform-app-client/build.rs`), so an omitted manifest key leaves the
/// remote allowlist unchanged while `Some(vec![])` clears it.
fn update_input(
    fields: UpdateFields,
    redirect_uris: Option<Vec<String>>,
) -> MutationUpdateApplicationInput {
    MutationUpdateApplicationInput {
        label: fields.label,
        description: fields.description,
        distribution_type: fields.distribution_type.map(Into::into),
        redirect_uris,
        authorization_scopes: None,
        name: None,
        proxy_url: None,
        url: None,
    }
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
                "Update the label, description, or distribution type of a \
                GoDaddy developer-platform application by its ID. At least one \
                of --label, --description, or --distribution-type must be \
                provided. Use `gddy platform app info --name <name>` to \
                retrieve the application ID. `--distribution-type private` \
                limits the application to your organization and deactivates \
                its active marketplace listing; `public` makes it installable \
                by anyone. When the environment's godaddy.toml declares \
                redirect_uris, the same request synchronizes those additional \
                OAuth callbacks; omitting the key leaves them unchanged.",
            )
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .with_scopes(&[APP_REGISTRY_READ, APP_REGISTRY_WRITE])
            .with_output_schema::<ApplicationUpdate>(),
        |context, args: UpdateArgs| async move {
            let config_path = crate::config::config_path(Some(&context.middleware.env));
            let redirect_uris = match crate::config::read_config(&config_path) {
                Ok(config) => config.redirect_uris,
                Err(crate::config::ConfigError::NotFound { .. }) => None,
                Err(error) => {
                    return Err(crate::error::GddyError::config(format!(
                        "invalid config: {error}"
                    ))
                    .into_cli_error());
                }
            };
            let input = update_input(args.fields, redirect_uris);
            let client = super::make_client(&context).await?;
            let app = client
                .update_application(&args.id, input)
                .await
                .map_err(super::client_err)?
                .ok_or_else(|| {
                    crate::error::GddyError::unexpected(
                        "updateApplication returned no data".to_owned(),
                    )
                    .into_cli_error()
                })?;
            let name = app.name.clone();
            let data = serde_json::to_value(&app).map_err(|e| {
                crate::error::GddyError::unexpected(format!("failed to encode application: {e}"))
                    .into_cli_error()
            })?;
            Ok(CommandResult::new(data).with_next_actions(vec![
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
            ]))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{DistributionTypeArg, UpdateFields, update_input};

    fn update_clap_command() -> clap::Command {
        super::command().spec.clap_command()
    }

    #[test]
    fn update_accepts_distribution_type_alone() {
        for value in ["public", "private"] {
            update_clap_command()
                .try_get_matches_from(["update", "--id", "app-1", "--distribution-type", value])
                .expect("public and private alone must be accepted");
        }
    }

    #[test]
    fn update_rejects_custom_and_unknown_distribution_types() {
        for value in ["custom", "unlisted"] {
            let err = update_clap_command()
                .try_get_matches_from(["update", "--id", "app-1", "--distribution-type", value])
                .expect_err("value must be rejected at parse time");
            assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue, "{err}");
        }
    }

    #[test]
    fn type_only_update_sends_just_the_distribution_type() {
        let fields = UpdateFields {
            label: None,
            description: None,
            distribution_type: Some(DistributionTypeArg::Public),
        };
        let wire = serde_json::to_value(update_input(fields, None)).expect("serialize");
        assert_eq!(wire, serde_json::json!({ "distributionType": "PUBLIC" }));
    }

    #[test]
    fn update_without_distribution_type_leaves_it_out_of_the_request() {
        let fields = UpdateFields {
            label: Some("New".to_owned()),
            description: None,
            distribution_type: None,
        };
        let wire = serde_json::to_value(update_input(fields, None)).expect("serialize");
        assert_eq!(wire, serde_json::json!({ "label": "New" }));
    }

    #[test]
    fn status_is_an_unsupported_argument() {
        let err = update_clap_command()
            .try_get_matches_from(["update", "--id", "app-1", "--status", "ACTIVE"])
            .expect_err("--status must not be accepted by app update");
        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
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
            ])
            .expect("multiple update fields should be allowed together");
    }
}
