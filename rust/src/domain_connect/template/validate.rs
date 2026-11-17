//! `gddy domain-connect template validate` — check a template without side effects.

use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};

use super::common::{human_validate, make_client, read_template, result_json};
use crate::output_schema::output_schema;
use crate::scopes::DOMAIN_CONNECT_TEMPLATE_READ;

output_schema!(ValidateOutput {
    "file": "string";
    "providerId": "string";
    "serviceId": "string";
    "action": "string";
    "outcome": "string";
    "operation": "string", optional;
    "issues": "[]object";
    "summary": "string", optional;
});

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Template file to validate
    #[arg(value_name = "FILE")]
    file: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("validate", "Validate a Domain Connect template")
            .with_long(
                "Check a template against the Domain Connect schema, the dc-template-linter \
                 and GoDaddy's provider-ownership rules. Nothing is stored or submitted.\n\
                 \n\
                 The outcome is one of:\n\
                   READY    the template can be submitted\n\
                   INVALID  schema or linter findings; see issues\n\
                   BLOCKED  ownership or version problem; see issues\n\
                 \n\
                 Example:\n\
                   gddy domain-connect template validate acme.com.managed-wordpress.json",
            )
            .with_system("domain-connect")
            .with_tier(Tier::Read)
            .with_scopes(&[DOMAIN_CONNECT_TEMPLATE_READ])
            .with_output_schema::<ValidateOutput>(),
        |ctx, args: Args| async move {
            let (path, template) = read_template(&args.file)?;
            let client = make_client(&ctx).await?;
            let result = client.validate(&template).await?;

            let data = result_json(&path, &template, &result);
            let output = if ctx.middleware.output_format == "human" {
                human_validate(&path, &result, data)
            } else {
                data
            };
            Ok(CommandResult::new(output))
        },
    )
}
