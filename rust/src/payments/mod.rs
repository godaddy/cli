//! `gddy payments` — payment method management.

use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, GroupSpec, Module, RuntimeCommandSpec,
    RuntimeGroupSpec, Tier,
};
use serde_json::json;

use crate::environments;

fn map_env_err(e: environments::EnvError) -> CliCoreError {
    CliCoreError::message(e.to_string())
}

pub fn module() -> Module {
    Module::new("Payments", |_ctx| {
        RuntimeGroupSpec::new(GroupSpec::new("payments", "Manage payment methods"))
            .with_command(add_command())
    })
}

fn add_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new(
            "add",
            "Add a payment method to your GoDaddy account (opens browser)",
        )
        .with_long(
            "Opens your default browser to the GoDaddy payment methods management page.\n\
                 Note: only credit card or Good-as-Gold can be used for domain purchases.",
        )
        .with_system("payments")
        .with_tier(Tier::Mutate)
        .no_auth(true),
        |ctx| async move {
            let env = environments::resolve(&ctx.middleware.env).map_err(map_env_err)?;
            let url = format!("{}/payment-methods/add-payment?plid=1", env.account_url);
            let opened = open::that(&url).is_ok();
            Ok(CommandResult::new(json!({
                "url": url,
                "info": if opened {
                    "Browser opened. Only credit card or Good-as-Gold can be used for domain purchases."
                } else {
                    "Could not open browser. Visit the URL above to add a payment method. \
                     Only credit card or Good-as-Gold can be used for domain purchases."
                }
            })))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prod_account_url_via_environments_module() {
        let env = environments::resolve("prod").expect("prod resolves");
        assert_eq!(
            format!("{}/payment-methods/add-payment?plid=1", env.account_url),
            "https://account.godaddy.com/payment-methods/add-payment?plid=1"
        );
    }

    #[test]
    fn ote_account_url_via_environments_module() {
        let env = environments::resolve("ote").expect("ote resolves");
        assert_eq!(
            format!("{}/payment-methods/add-payment?plid=1", env.account_url),
            "https://account.ote-godaddy.com/payment-methods/add-payment?plid=1"
        );
    }
}
