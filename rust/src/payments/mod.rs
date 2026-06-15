//! `gddy payments` — payment method management.

use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, GroupSpec, Module, RuntimeCommandSpec,
    RuntimeGroupSpec, Tier,
};
use serde_json::json;

pub fn module() -> Module {
    Module::new("Payments", |_ctx| {
        RuntimeGroupSpec::new(GroupSpec::new("payments", "Manage payment methods"))
            .with_command(add_command())
    })
}

fn add_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("add", "Add a payment method to your GoDaddy account (opens browser)")
            .with_long(
                "Opens your default browser to the GoDaddy payment methods management page.\n\
                 Note: only credit card or Good-as-Gold can be used for domain purchases.",
            )
            .with_system("payments")
            .with_tier(Tier::Mutate)
            .no_auth(true),
        |ctx| async move {
            let url = account_url_for_env(&ctx.middleware.env);
            open::that(&url)
                .map_err(|e| CliCoreError::message(format!("failed to open browser: {e}")))?;
            Ok(CommandResult::new(json!({
                "info": "Browser opened to the payment methods management page. \
                         Only credit card or Good-as-Gold can be used for domain purchases."
            })))
        },
    )
}

fn account_url_for_env(env: &str) -> String {
    let host = if env == "prod" {
        "account.godaddy.com".to_owned()
    } else {
        format!("account.{env}-godaddy.com")
    };
    format!("https://{host}/payment-methods/add-payment?plid=1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prod_uses_bare_godaddy_domain() {
        assert_eq!(
            account_url_for_env("prod"),
            "https://account.godaddy.com/payment-methods/add-payment?plid=1"
        );
    }

    #[test]
    fn ote_inserts_env_prefix() {
        assert_eq!(
            account_url_for_env("ote"),
            "https://account.ote-godaddy.com/payment-methods/add-payment?plid=1"
        );
    }

    #[test]
    fn dev_inserts_env_prefix() {
        assert_eq!(
            account_url_for_env("dev"),
            "https://account.dev-godaddy.com/payment-methods/add-payment?plid=1"
        );
    }

    #[test]
    fn test_inserts_env_prefix() {
        assert_eq!(
            account_url_for_env("test"),
            "https://account.test-godaddy.com/payment-methods/add-payment?plid=1"
        );
    }
}
