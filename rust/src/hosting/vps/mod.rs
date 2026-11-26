mod get;
mod list;
mod operation;
mod power;
mod restart;
mod start;
mod stop;

use cli_engine::{GroupSpec, RuntimeGroupSpec, Stage};

use crate::hosting::common::VPS_FLAG;

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("vps", "Manage virtual private servers")
            .with_long(
                "Work with the virtual private servers in your account. Servers are \
                 separate from hosting applications: they have no app type and \
                 `hosting app list` does not show them. Start with `hosting vps list` \
                 to see your servers, then `hosting vps get` for one server's status.",
            )
            .with_feature_flag(VPS_FLAG, Stage::Experimental),
    )
    .with_command(list::command())
    .with_command(get::command())
    .with_command(start::command())
    .with_command(stop::command())
    .with_command(restart::command())
    .with_group(operation::group())
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig, Stage};

    use crate::hosting::common::VPS_FLAG;
    use crate::hosting::test_support::{cli, run_hosting};

    fn cli_with_vps() -> Cli {
        Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_min_stage(Stage::Ga)
                .with_default_auth_provider("godaddy")
                .with_feature_override(VPS_FLAG, Stage::Ga)
                .with_module(crate::hosting::module()),
        )
    }

    /// The subcommand list in `hosting --help` (after "Commands:"), which the
    /// engine builds from the registered groups.
    fn commands_section(help: &str) -> &str {
        help.split("Commands:").nth(1).unwrap_or_default()
    }

    #[tokio::test]
    async fn vps_is_hidden_at_the_default_stage() {
        let output = run_hosting(&cli(), &["vps", "--help"]).await;
        assert!(
            !output.contains("virtual private servers"),
            "vps should be hidden without its flag: {output}"
        );
        let hosting = run_hosting(&cli(), &["--help"]).await;
        assert!(!commands_section(&hosting).contains("vps"), "{hosting}");
    }

    #[tokio::test]
    async fn vps_is_visible_with_its_flag() {
        let cli = cli_with_vps();
        let help = run_hosting(&cli, &["vps", "--help"]).await;
        assert!(help.contains("virtual private servers"), "{help}");
        assert!(help.contains("list"), "{help}");
        let hosting = run_hosting(&cli, &["--help"]).await;
        assert!(commands_section(&hosting).contains("vps"), "{hosting}");
    }

    #[tokio::test]
    async fn every_vps_command_is_registered() {
        let cli = cli_with_vps();
        for command in ["list", "get", "start", "stop", "restart", "operation"] {
            let help = run_hosting(&cli, &["vps", "--help"]).await;
            assert!(help.contains(command), "missing {command}: {help}");
        }
        let help = run_hosting(&cli, &["vps", "operation", "--help"]).await;
        assert!(help.contains("get"), "{help}");
    }

    #[tokio::test]
    async fn stop_and_restart_refuse_to_run_without_confirm() {
        let cli = cli_with_vps();
        for command in ["stop", "restart"] {
            let out = run_hosting(
                &cli,
                &["vps", command, "--server-id", "VPS-phx3-a", "--help"],
            )
            .await;
            assert!(out.contains("--confirm"), "{command}: {out}");
            assert!(out.contains("--no-wait"), "{command}: {out}");
        }
        let start = run_hosting(&cli, &["vps", "start", "--help"]).await;
        assert!(!start.contains("--confirm"), "{start}");
    }

    #[tokio::test]
    async fn list_help_names_its_fields_and_takes_no_app_type() {
        let help = run_hosting(&cli_with_vps(), &["vps", "list", "--help"]).await;
        assert!(help.contains("--limit"), "{help}");
        assert!(!help.contains("--app-type"), "{help}");
        for field in [
            "name",
            "id",
            "status",
            "os",
            "controlPanel",
            "createdAt",
            "expiresAt",
        ] {
            assert!(help.contains(field), "missing {field}: {help}");
        }
    }
}
