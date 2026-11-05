//! `--currency`, shared by the shopping commands that price products.

use cli_engine::CommandContext;

use super::common::currency_code;
use crate::error::GddyError;
use crate::prefs::Prefs;

/// Precedence: the flag, then `GDDY_CURRENCY`, then the saved `currency`
/// preference, then the catalog's own default.
#[derive(Debug, Clone, Default, clap::Args)]
pub(crate) struct CurrencyArgs {
    /// Currency to show prices in, as an ISO 4217 code (for example, USD or GBP). Set a default
    /// with `gddy config set currency <CODE>`.
    #[arg(long, env = "GDDY_CURRENCY", value_name = "CODE", value_parser = currency_code)]
    currency: Option<String>,
}

impl CurrencyArgs {
    /// The currency to request. The config file is only read when neither the
    /// flag nor the environment variable decided, so an explicit choice works
    /// even while the file is broken.
    pub(crate) fn resolve(self, ctx: &CommandContext) -> Result<Option<String>, GddyError> {
        match self.currency {
            Some(explicit) => Ok(Some(explicit)),
            None => Ok(Prefs::load(ctx)?.currency),
        }
    }
}

#[cfg(test)]
mod tests {
    use cli_engine::{
        Cli, CliConfig, CommandResult, CommandSpec, GroupSpec, Module, RuntimeCommandSpec,
        RuntimeGroupSpec,
    };
    use serde_json::json;

    use super::*;
    use crate::environments::test_support::{ENV_LOCK, EnvGuard};

    #[derive(Debug, Clone, clap::Args)]
    struct ShowArgs {
        #[command(flatten)]
        currency: CurrencyArgs,
    }

    fn cli() -> Cli {
        let module = Module::new("Test", |_ctx| {
            RuntimeGroupSpec::new(GroupSpec::new("t", "test")).with_command(
                RuntimeCommandSpec::new_typed_with_context::<ShowArgs, _, _, _>(
                    CommandSpec::from_args::<ShowArgs>("show", "show").no_auth(true),
                    |ctx, args: ShowArgs| async move {
                        let currency = args.currency.resolve(&ctx)?;
                        Ok(CommandResult::new(json!({ "currency": currency })))
                    },
                ),
            )
        });
        Cli::new(
            CliConfig::new("gddy", "test", "gddy")
                .with_config_schema::<Prefs>()
                .with_startup_args(Vec::<&str>::new())
                .with_module(module),
        )
    }

    /// Runs `gddy t show <flag>` with the given saved config and env var.
    async fn run(config: Option<&str>, env: Option<&str>, flag: Option<&str>) -> (i32, String) {
        let _lock = ENV_LOCK.lock().await;
        let dir = tempfile::tempdir().expect("tempdir");
        let _xdg = EnvGuard::set("XDG_CONFIG_HOME", &dir.path().display().to_string());
        let _env = match env {
            Some(value) => EnvGuard::set("GDDY_CURRENCY", value),
            None => EnvGuard::unset("GDDY_CURRENCY"),
        };
        if let Some(contents) = config {
            let path = dir.path().join("gddy");
            std::fs::create_dir_all(&path).expect("mkdir");
            std::fs::write(path.join("config.toml"), contents).expect("write config");
        }
        let mut args = vec!["gddy", "t", "show"];
        args.extend(flag.into_iter().flat_map(|f| f.split(' ')));
        let out = cli().run(args).await;
        (out.exit_code, out.rendered)
    }

    async fn currency(config: Option<&str>, env: Option<&str>, flag: Option<&str>) -> String {
        let (code, rendered) = run(config, env, flag).await;
        assert_eq!(
            code, 0,
            "config={config:?} env={env:?} flag={flag:?}: {rendered}"
        );
        let start = rendered.find("\"currency\":").expect("currency key");
        rendered[start..]
            .lines()
            .next()
            .expect("line")
            .trim_end_matches(',')
            .to_owned()
    }

    const SAVED: Option<&str> = Some("currency = \"GBP\"\n");

    #[tokio::test]
    async fn unset_by_default() {
        assert_eq!(currency(None, None, None).await, "\"currency\": null");
    }

    #[tokio::test]
    async fn saved_preference_applies_in_canonical_case() {
        assert_eq!(currency(SAVED, None, None).await, "\"currency\": \"GBP\"");
        assert_eq!(
            currency(Some("currency = \"eur\"\n"), None, None).await,
            "\"currency\": \"EUR\""
        );
    }

    #[tokio::test]
    async fn env_beats_saved_preference() {
        assert_eq!(
            currency(SAVED, Some("jpy"), None).await,
            "\"currency\": \"JPY\""
        );
    }

    #[tokio::test]
    async fn flag_beats_env_and_saved_preference() {
        assert_eq!(
            currency(SAVED, Some("jpy"), Some("--currency usd")).await,
            "\"currency\": \"USD\""
        );
    }

    #[tokio::test]
    async fn explicit_choice_works_while_the_config_file_is_broken() {
        let broken = Some("not = valid = toml");
        assert_eq!(
            currency(broken, None, Some("--currency usd")).await,
            "\"currency\": \"USD\""
        );
    }

    #[tokio::test]
    async fn broken_saved_currency_is_an_error_not_a_silent_default() {
        let (code, rendered) = run(Some("currency = \"banana\"\n"), None, None).await;
        assert_ne!(code, 0, "{rendered}");
        assert!(rendered.contains("fix"), "{rendered}");
    }
}
