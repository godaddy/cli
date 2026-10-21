//! `gddy platform app onboard` — accept the GoDaddy Developer agreements and
//! finish developer account setup.

use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::json;

use super::schemas::ApplicationOnboard;
use crate::next_action::next_action;
use crate::platform::app::onboarding::{
    OnboardingPreview, ensure_ready_for_app_init, preview_onboarding,
};
use crate::scopes::{APP_REGISTRY_READ, APP_REGISTRY_WRITE};

#[derive(Debug, Clone, clap::Args)]
struct OnboardArgs {
    /// Accept the GoDaddy Developer agreements without a prompt. Required when
    /// not running in a terminal; in a terminal you're always asked to confirm.
    #[arg(long)]
    accept_agreements: bool,
}

fn follow_ups() -> Vec<cli_engine::NextAction> {
    vec![
        next_action(
            "platform app init --name <name> --description <description> --url <url> --proxy-url <proxy-url> --scopes <scopes>",
            "Create your first application",
        )
        .with_param("name", NextActionParam::required())
        .with_param("description", NextActionParam::required())
        .with_param("url", NextActionParam::required())
        .with_param("proxy-url", NextActionParam::required())
        .with_param("scopes", NextActionParam::required()),
        next_action("platform app list", "List your applications"),
    ]
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<OnboardArgs, _, _, _>(
        CommandSpec::from_args::<OnboardArgs>(
            "onboard",
            "Accept the developer agreements and finish account setup",
        )
        .with_long(
            "Finish setting up your GoDaddy developer account by accepting the \
            GoDaddy Developer agreements. You'll be shown the agreements and \
            asked to press Enter to accept. Run this once before using other \
            `gddy platform app` commands. When not running in a terminal (for \
            example in a script), pass `--accept-agreements` to accept without \
            a prompt. Use `--dry-run` to check whether your account is set up \
            without accepting anything; checking may start your account's setup \
            record, but no agreement is accepted.\n\n\
            AI assistants: accepting the agreements is the user's decision. Ask \
            the user to review and accept them; do not pass \
            `--accept-agreements` on their behalf.",
        )
        .with_system("applications")
        .with_tier(Tier::Mutate)
        .mutates(true)
        .handles_dry_run(true)
        .with_scopes(&[APP_REGISTRY_READ, APP_REGISTRY_WRITE])
        .with_output_schema::<ApplicationOnboard>(),
        |ctx, args: OnboardArgs| async move {
            let env = ctx.middleware.env.clone();
            let token = ctx.credential().await?.token;

            if ctx.dry_run() {
                let (org_id, status, already, action) =
                    match preview_onboarding(&token, &env).await? {
                        OnboardingPreview::AlreadyComplete { org_id } => {
                            (org_id, "ACTIVE", true, "dry-run: already set up")
                        }
                        OnboardingPreview::WouldComplete { org_id } => (
                            org_id,
                            "PENDING",
                            false,
                            "dry-run: would accept the developer agreements and finish setup",
                        ),
                    };
                return Ok(CommandResult::new(json!({
                    "organizationId": org_id,
                    "status": status,
                    "alreadyOnboarded": already,
                    "action": action,
                }))
                .with_dry_run());
            }

            let outcome = ensure_ready_for_app_init(&token, &env, args.accept_agreements).await?;
            let action = if outcome.newly_onboarded {
                "accepted the developer agreements and finished setup"
            } else {
                "already set up"
            };
            Ok(CommandResult::new(json!({
                "organizationId": outcome.org_id,
                "status": "ACTIVE",
                "alreadyOnboarded": !outcome.newly_onboarded,
                "action": action,
            }))
            .with_next_actions(follow_ups()))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{command, follow_ups};

    #[test]
    fn command_is_mutating_and_handles_its_own_dry_run() {
        let spec = command().spec;
        assert!(spec.mutates);
        assert!(spec.handles_dry_run);
    }

    #[test]
    fn help_tells_ai_assistants_not_to_accept_on_the_users_behalf() {
        let long = command().spec.long.expect("long help");
        assert!(long.contains("AI assistants:"));
        assert!(long.contains("do not pass `--accept-agreements`"));
    }

    #[test]
    fn every_placeholder_in_the_init_follow_up_is_a_required_param() {
        let actions = follow_ups();
        let init = &actions[0];
        for name in ["name", "description", "url", "proxy-url", "scopes"] {
            assert!(
                init.command.contains(&format!("<{name}>")),
                "template is missing <{name}>"
            );
            assert!(init.params[name].required, "{name} must be required");
        }
    }

    #[test]
    fn help_does_not_promise_a_prompt_free_run_in_a_terminal() {
        let long = command().spec.long.expect("long help");
        assert!(long.contains("When not running in a terminal"));
        assert!(!long.contains("In scripts, pass"));
    }

    #[test]
    fn help_explains_what_a_dry_run_does_and_does_not_do() {
        let long = command().spec.long.expect("long help");
        assert!(long.contains("`--dry-run`"));
        assert!(long.contains("no agreement is accepted"));
    }
}
