//! `gddy payment-methods` — payment method management.

use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, Module, NextActionParam, RuntimeCommandSpec,
    RuntimeGroupSpec, Tier,
};
use serde_json::json;

use crate::environments;
use crate::next_action::next_action;

pub fn module() -> Module {
    Module::new("Payment Methods", |_ctx| {
        RuntimeGroupSpec::new(
            GroupSpec::new("payment-methods", "Manage payment methods").with_long(
                "Manage the payment methods on your GoDaddy account.\n\
                     A saved payment method is required before you can run \
                     `gddy domain purchase`.\n\
                     Use `gddy payment-methods add` to open the GoDaddy payment methods \
                     page in your browser.",
            ),
        )
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
        .with_system("payment-methods")
        .with_tier(Tier::Mutate)
        .no_auth(true),
        |ctx| async move {
            let env = environments::resolve(&ctx.middleware.env)?;
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
            }))
            .with_next_actions(vec![
                next_action(
                    "domain quote <domain>",
                    "Price a domain registration now that a payment method is on file",
                )
                .with_param("domain", NextActionParam::required()),
            ]))
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
