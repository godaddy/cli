pub mod client;
mod common;

mod check_eligibility;
mod create;
mod get;
mod list;

pub(crate) use common::{client_err, client_err_with_fix, make_client};

use cli_engine::{GroupSpec, Module, RuntimeGroupSpec, Stage};

pub fn module() -> Module {
    Module::new("Email", |_ctx| {
        RuntimeGroupSpec::new(
            GroupSpec::new("email", "Create, list, and inspect GoDaddy Email mailboxes").with_long(
                "Manage GoDaddy Email mailboxes over panel-v3.\n\
             \n\
             • check-eligibility — see which account(s) an address can be created\n\
             \x20  under, and what consent is outstanding\n\
             • create            — provision a mailbox\n\
             • list / get        — your existing mailboxes and their details\n\
             \n\
             See `gddy guide email-mailboxes` for what an account ID is and how the\n\
             check-eligibility → create flow works.",
            ),
        )
        .with_command(list::command())
        .with_command(get::command())
        .with_command(create::command())
        .with_command(check_eligibility::command())
    })
    .with_feature_flag("email", Stage::Beta)
    .with_guides_from_markdown([(
        "email-mailboxes.md",
        include_bytes!("guides/email-mailboxes.md").as_slice(),
    )])
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig, Stage};

    #[tokio::test]
    async fn email_commands_require_auth() {
        const AUTH_FAILURE_EXIT: i32 = 2;
        let cases: [&[&str]; 4] = [
            &["gddy", "email", "list", "--output", "json"],
            &["gddy", "email", "get", "mbx-456", "--output", "json"],
            &[
                "gddy",
                "email",
                "create",
                "--email",
                "someone@example.com",
                "--output",
                "json",
            ],
            &[
                "gddy",
                "email",
                "check-eligibility",
                "--email",
                "someone@example.com",
                "--output",
                "json",
            ],
        ];

        for args in cases {
            let cli = Cli::new(
                CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                    .with_min_stage(Stage::Beta)
                    .with_default_auth_provider("godaddy")
                    .with_module(super::module()),
            );
            let output = cli.run(args.iter().copied()).await;
            assert_eq!(
                output.exit_code, AUTH_FAILURE_EXIT,
                "args {args:?} -> {output:?}"
            );
            let json: serde_json::Value =
                serde_json::from_str(&output.rendered).expect("valid json output");
            let message = json["error"]["message"].as_str().unwrap_or_default();
            assert!(message.contains("provider"), "args {args:?} -> {message:?}");
        }
    }
}
