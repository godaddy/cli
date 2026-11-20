//! `gddy domain-connect template validate` — check a template without side effects.

use cli_engine::{
    CommandResult, CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, Tier,
};

use super::common::{
    READY, SUBMIT_CONFIRMATION_NOTE, SYSTEM, read_template, result_json, validate, validate_columns,
};
use crate::next_action::next_action;
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
});

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Template file written by `template init`, e.g. acme.com.email.json
    #[arg(value_name = "FILE")]
    file: String,
}

/// Offered only when the template is ready, and worded for whoever reads it.
fn submit_action(file: &str, show_agent_note: bool) -> NextAction {
    let description = if show_agent_note {
        format!("Submit the template when you are ready to publish it. {SUBMIT_CONFIRMATION_NOTE}")
    } else {
        "Submit the template when you are ready to publish it".to_owned()
    };
    next_action("domain-connect template submit <file>", description)
        .with_param("file", NextActionParam::value(file))
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("validate", "Check a Domain Connect template for problems")
            .with_long(
                "Check a template file against the Domain Connect template rules and \
                 confirm that you can publish under its provider ID. Nothing is stored or \
                 submitted, so you can run this as often as you like while editing.\n\
                 \n\
                 The outcome is one of:\n\
                   READY    the template can be submitted\n\
                   INVALID  the template has problems; each issue names the field and a fix\n\
                   BLOCKED  the provider ID belongs to someone else, or the version must be raised\n\
                 \n\
                 Example:\n\
                   gddy domain-connect template validate acme.com.email.json",
            )
            .with_system(SYSTEM)
            .with_tier(Tier::Read)
            .with_scopes(&[DOMAIN_CONNECT_TEMPLATE_READ])
            .with_output_schema::<ValidateOutput>()
            .with_view(validate_columns()),
        |ctx, args: Args| async move {
            let (path, template) = read_template(&args.file)?;
            let client = crate::domain::common::make_client(&ctx).await?;
            let result = validate(&client, &template).await?;

            let data = result_json(&path, &template, &result);
            let actions = if result.outcome.as_str() == READY {
                vec![submit_action(
                    &path.display().to_string(),
                    ctx.middleware.output_format != "human",
                )]
            } else {
                Vec::new()
            };
            Ok(CommandResult::new(data).with_next_actions(actions))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_submit_action_carries_the_file_as_a_parameter() {
        let action = submit_action("/work/it's \"mine\".json", true);

        assert_eq!(action.command, "gddy domain-connect template submit <file>");
        assert_eq!(
            action.params["file"].value.as_deref(),
            Some("/work/it's \"mine\".json")
        );
    }

    #[test]
    fn only_assistants_are_told_to_confirm_before_submitting() {
        let for_agent = submit_action("t.json", true);
        assert!(for_agent.description.contains("AI assistants:"));
        assert!(for_agent.description.contains("explicit confirmation"));

        let for_human = submit_action("t.json", false);
        assert!(!for_human.description.contains("AI assistants:"));
    }

    #[test]
    fn validate_reads_but_never_writes() {
        let meta = command().spec.metadata();
        assert_eq!(meta.scopes, [DOMAIN_CONNECT_TEMPLATE_READ]);
        assert!(!meta.dry_run_prompt);
    }
}
