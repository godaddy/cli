//! `gddy domain-connect template submit` — validate and submit a template.

use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};

use super::client::outcome;
use super::common::{human_submit, issue_lines, make_client, read_template, result_json};
use crate::error::GddyError;
use crate::output_schema::output_schema;
use crate::scopes::{DOMAIN_CONNECT_TEMPLATE_READ, DOMAIN_CONNECT_TEMPLATE_WRITE};

output_schema!(SubmitOutput {
    "file": "string";
    "providerId": "string";
    "serviceId": "string";
    "action": "string";
    "outcome": "string";
    "operation": "string", optional;
    "issues": "[]object";
    "summary": "string", optional;
    "pr": "object", optional;
});

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Template file to submit
    #[arg(value_name = "FILE")]
    file: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("submit", "Submit a Domain Connect template")
            .with_long(
                "Validate a template and, when it is ready, submit it to GoDaddy. GoDaddy DNS \
                 stores the template, the provider is claimed for your account, and a pull \
                 request is opened (or your existing one updated) on the public \
                 Domain-Connect/Templates repository. The pull request URL is printed; track \
                 progress there.\n\
                 \n\
                 A template that is INVALID or BLOCKED is not submitted; the issues are printed \
                 and the command exits non-zero. Run `template validate` first to iterate.\n\
                 \n\
                 Example:\n\
                   gddy domain-connect template submit acme.com.managed-wordpress.json",
            )
            .with_system("domain-connect")
            .with_tier(Tier::Mutate)
            .with_scopes(&[DOMAIN_CONNECT_TEMPLATE_READ, DOMAIN_CONNECT_TEMPLATE_WRITE])
            .with_output_schema::<SubmitOutput>(),
        |ctx, args: Args| async move {
            let (path, template) = read_template(&args.file)?;
            let client = make_client(&ctx).await?;
            let result = client.submit(&template).await?;

            if result.outcome != outcome::SUBMITTED {
                let mut message = format!("{}: {}", path.display(), result.outcome);
                for line in issue_lines(&result) {
                    message.push('\n');
                    message.push_str(&line);
                }
                let error = GddyError::validation(message).with_system("domain-connect");
                let error = if result.outcome == outcome::BLOCKED {
                    error.with_fix(
                        "Use a providerId you own, raise the template version when modifying an \
                         existing service, or contact #domains-dcc if the provider exists but is unclaimed.",
                    )
                } else {
                    error.with_fix("Fix the listed issues and run `gddy domain-connect template validate` until READY.")
                };
                return Err(error.into_cli_error());
            }

            let data = result_json(&path, &template, &result);
            let output = if ctx.middleware.output_format == "human" {
                human_submit(&path, &result, data)
            } else {
                data
            };
            Ok(CommandResult::new(output))
        },
    )
}
