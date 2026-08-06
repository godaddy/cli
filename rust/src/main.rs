mod actions_catalog;
mod api_explorer;
mod application;
mod auth;
mod config;
mod contacts;
mod dns;
mod domain;
mod env;
mod environments;
mod error;
mod extension;
mod hosting;
mod next_action;
pub mod onboarding;
mod output_schema;
mod pat;
mod payment_methods;
mod platform;
mod quote_cache;
mod scopes;
mod scopes_cmd;
mod summary;
mod update;
mod webhook;

use std::{process::ExitCode, sync::Arc};

use cli_engine::{BuildInfo, Cli, CliConfig, Module};

use crate::next_action::next_action;

/// The full set of `gddy` command modules, shared between `main`'s
/// [`CliConfig`] wiring and anything that needs to walk the real command
/// tree standalone (e.g. `auth scopes`'s live scope→command correlation via
/// [`cli_engine::build_module_group`]), so both draw from exactly one list.
pub(crate) fn all_modules() -> Vec<Module> {
    vec![
        api_explorer::module(),
        dns::module(),
        domain::module(),
        env::module(),
        hosting::module(),
        pat::module(),
        payment_methods::module(),
        platform::module(),
        update::module(),
    ]
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let auth_provider = Arc::new(auth::GoDaddyAuthProvider::new());

    let cli = Cli::new(
        CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
            .with_long(
                "gddy is the command-line interface to the GoDaddy developer platform.\n\
                 \n\
                 What you can do:\n  \
                 • domain   — list your domains, check availability, get suggestions, and register new ones\n  \
                 • dns      — view and edit a domain's DNS records\n  \
                 • api      — explore and call GoDaddy REST API endpoints directly\n  \
                 • platform — build and manage GoDaddy Platform integrations\n  \
                 • hosting  — manage Node.js PaaS applications (create, upload, deploy)\n  \
                 • payment-methods — manage the payment methods used for purchases\n\
                 \n\
                 Most commands need authentication; run `gddy auth login` first, or use a PAT via `gddy pat add` / `GDDY_PAT` for non-interactive workflows (or just run a\n\
                 command and follow the prompt). Use `--env` to target an environment and\n\
                 `gddy tree` to see every command. New here? Try `gddy domain available <name>`.",
            )
            .with_build(
                BuildInfo::new(env!("CARGO_PKG_VERSION"))
                    .with_commit(env!("GDDY_BUILD_COMMIT"))
                    .with_date(env!("GDDY_BUILD_DATE")),
            )
            .with_default_auth_provider("godaddy")
            .with_auth_provider(auth_provider)
            .with_auth_extra_commands([scopes_cmd::auth_scopes_command()])
            .with_min_stage(cli_engine::Stage::Ga)
            .with_environments(Arc::clone(environments::instance()))
            .with_root_next_actions(Arc::new(|| {
                vec![
                    next_action("auth status", "Check authentication status"),
                    next_action("env get", "Get the current active environment"),
                    next_action("tree", "Display the full command tree"),
                ]
            }))
            .with_pre_run(Arc::new(|_mw, _path, _args| {
                update::maybe_spawn_background_refresh();
                Ok(())
            }))
            .with_on_shutdown(Arc::new(update::maybe_print_update_notice))
            .with_modules(all_modules()),
    );

    cli.execute().await
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cli_engine::{Cli, CliConfig, Stage, environments::EnvTable};

    /// Regression test for a real bug found while wiring feature-flagging up
    /// properly: with no `min_stage` override anywhere, cli-engine's own
    /// default (`Stage::Ga`) hides `hosting` and the Developer Platform
    /// namespace entirely. Guards that the *global* default stays `Ga` per
    /// product decision — i.e. these stay hidden absent an environment override.
    #[tokio::test]
    async fn beta_and_experimental_modules_stay_hidden_at_the_default_min_stage() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_min_stage(Stage::Ga)
                .with_modules(super::all_modules()),
        );
        let output = cli.run(["gddy", "hosting", "--help"]).await;
        assert_ne!(
            output.exit_code, 0,
            "hosting should stay hidden at the Ga default: {}",
            output.rendered
        );
    }

    #[tokio::test]
    async fn platform_namespace_is_hidden_at_the_default_min_stage() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_min_stage(Stage::Ga)
                .with_modules(super::all_modules()),
        );
        let output = cli.run(["gddy", "platform", "--help"]).await;
        assert_ne!(
            output.exit_code, 0,
            "platform should stay hidden at the Ga default: {}",
            output.rendered
        );
    }

    /// The other half of the guard: an environment whose resolved
    /// `min_stage` is lower than the global default reveals those same
    /// modules. This is exactly the mechanism `environments.toml`'s
    /// `min_stage`/`feature_overrides` keys (or `<ENV>_MIN_STAGE`/
    /// `<ENV>_FEATURE_<KEY>` env vars) are meant to drive — see
    /// `crate::environments`'s module doc.
    #[tokio::test]
    async fn an_environment_min_stage_override_reveals_beta_and_experimental_modules() {
        let environments = Arc::new(
            cli_engine::environments::Environments::new("dev")
                .with_environment("dev", EnvTable::new().with("min_stage", "experimental")),
        );
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_min_stage(Stage::Ga)
                .with_environments(environments)
                .with_startup_args(Vec::<&str>::new())
                .with_modules(super::all_modules()),
        );
        let output = cli.run(["gddy", "hosting", "--help"]).await;
        assert_eq!(
            output.exit_code, 0,
            "hosting should be revealed under an Experimental-min_stage environment: {}",
            output.rendered
        );
    }

    #[tokio::test]
    async fn platform_namespace_exposes_the_gpa_command_tree() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_min_stage(Stage::Experimental)
                .with_modules(super::all_modules()),
        );

        let tree = cli.run(["gddy", "tree", "--output", "json"]).await;
        assert_eq!(tree.exit_code, 0, "{}", tree.rendered);
        for path in [
            "platform",
            "platform app",
            "platform actions",
            "platform webhook",
        ] {
            assert!(
                tree.rendered.contains(path),
                "tree should publish {path:?}: {}",
                tree.rendered
            );
        }

        for (command, child) in [
            (["gddy", "platform", "app", "--help"].as_slice(), "init"),
            (
                ["gddy", "platform", "actions", "--help"].as_slice(),
                "describe",
            ),
            (
                ["gddy", "platform", "webhook", "--help"].as_slice(),
                "events",
            ),
            (
                ["gddy", "platform", "application", "--help"].as_slice(),
                "init",
            ),
        ] {
            let output = cli.run(command).await;
            assert_eq!(output.exit_code, 0, "{}", output.rendered);
            assert!(
                output.rendered.contains(child),
                "help should list {child:?}: {}",
                output.rendered
            );
        }

        for legacy_root in ["application", "actions", "webhook"] {
            let output = cli.run(["gddy", legacy_root, "--help"]).await;
            assert_ne!(
                output.exit_code, 0,
                "legacy root {legacy_root:?} should not be registered: {}",
                output.rendered
            );
        }
    }
}
