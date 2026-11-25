//! Interactive prompts for `template init`, asked through the engine's
//! [`Prompter`] so they share cancel, validation and testing behaviour with
//! every other interactive command.

use std::sync::Arc;

use cli_engine::prompt::{Prompter, TextValidator, prompt_repeat};

use super::parsers::{self, Field, Need};
use super::types::{Record, RecordKind, Template};
use crate::error::GddyError;

fn nonempty(input: &str) -> Result<(), String> {
    if input.is_empty() {
        Err("A value is required.".to_owned())
    } else {
        Ok(())
    }
}

/// Offers to add DNS records until the user says no.
pub(super) fn prompt_records(
    prompter: &dyn Prompter,
    template: &mut Template,
) -> cli_engine::Result<()> {
    prompt_repeat(
        prompter,
        "Add a DNS record?",
        "Add another DNS record?",
        |prompter| {
            let kind = prompt_kind(prompter)?;
            template.records.push(prompt_record(prompter, kind)?);
            Ok(())
        },
    )
    .map(drop)
}

fn prompt_kind(prompter: &dyn Prompter) -> cli_engine::Result<RecordKind> {
    let options: Vec<String> = RecordKind::ALL
        .iter()
        .map(|kind| format!("{} ({})", kind.name(), kind.description()))
        .collect();
    let index = prompter.select("Record type", &options)?;
    RecordKind::ALL.get(index).copied().ok_or_else(|| {
        cli_engine::CliCoreError::message(format!("record type {index} is not in the list"))
    })
}

fn prompt_record(prompter: &dyn Prompter, kind: RecordKind) -> cli_engine::Result<Record> {
    let mut record = Record::new(kind);
    for field in parsers::fields(kind) {
        let (message, default) = field_prompt(field);
        let attr = field.attr;
        let accepts_blank = matches!(field.need, Need::Optional);
        let check: TextValidator = Arc::new(move |input: &str| {
            if input.is_empty() {
                return if accepts_blank {
                    Ok(())
                } else {
                    nonempty(input)
                };
            }
            // Try the value on a scratch record so a bad one is explained
            // and asked again instead of ending the session.
            parsers::set(&mut Record::new(kind), kind, attr, input.to_owned())
                .map_err(|problem| problem.to_string())
        });
        let answer = prompter.text(&message, default, Some(check))?;
        if answer.is_empty() {
            continue;
        }
        parsers::set(&mut record, kind, attr, answer).map_err(GddyError::into_cli_error)?;
    }
    Ok(record)
}

fn field_prompt(field: &Field) -> (String, Option<&'static str>) {
    match &field.need {
        Need::Required => (field.label.to_owned(), None),
        Need::Optional => (format!("{} (optional)", field.label), None),
        // The prompter shows the default itself; repeating it in the label
        // printed it twice.
        Need::Default(default) => (field.label.to_owned(), Some(*default)),
    }
}

#[cfg(test)]
mod tests {
    use cli_engine::prompt::{Answer, ScriptedPrompter};

    use super::*;
    use crate::domain_connect::template::types::{Essential, Number};

    fn texts(answers: &[&str]) -> Vec<Answer> {
        answers.iter().map(|answer| Answer::text(*answer)).collect()
    }

    fn template() -> Template {
        Template::from_basic_inputs(
            "acme.com".to_owned(),
            "Acme".to_owned(),
            "email".to_owned(),
            "Email".to_owned(),
            None,
        )
    }

    #[test]
    fn a_record_session_takes_defaults_for_blank_answers() {
        // One MX record (the fourth kind) taking the defaults for host,
        // priority and TTL, then no more records.
        let mut answers = vec![Answer::Confirm(true), Answer::Select(3)];
        answers.extend(texts(&["", "mail.acme.com", "", "", "mail", "OnApply"]));
        answers.push(Answer::Confirm(false));
        let prompter = ScriptedPrompter::new(answers);

        let mut template = template();
        prompt_records(&prompter, &mut template).expect("records");
        assert_eq!(template.records.len(), 1);
        let record = &template.records[0];
        assert_eq!(record.record_type, "MX");
        assert_eq!(record.host.as_deref(), Some("@"));
        assert_eq!(record.points_to.as_deref(), Some("mail.acme.com"));
        assert_eq!(record.priority, Some(Number::Literal(10)));
        assert_eq!(record.ttl, Some(Number::Literal(3600)));
        assert_eq!(record.group_id.as_deref(), Some("mail"));
        assert_eq!(record.essential, Some(Essential::OnApply));
        assert_eq!(prompter.remaining(), 0);
    }

    #[test]
    fn declining_the_first_offer_adds_no_records() {
        let prompter = ScriptedPrompter::new([Answer::Confirm(false)]);
        let mut template = template();
        prompt_records(&prompter, &mut template).expect("records");
        assert!(template.records.is_empty());
    }

    #[test]
    fn a_bad_record_value_is_asked_again_instead_of_ending_the_session() {
        let mut answers = vec![Answer::Confirm(true), Answer::Select(0)];
        // An A record: host, value, TTL ("soon" is refused, then 600), group,
        // essential ("No" is refused, then blank).
        answers.extend(texts(&["www", "192.0.2.1", "soon", "600", "", "No", ""]));
        answers.push(Answer::Confirm(false));
        let prompter = ScriptedPrompter::new(answers);

        let mut template = template();
        prompt_records(&prompter, &mut template).expect("records");
        let record = &template.records[0];
        assert_eq!(record.record_type, "A");
        assert_eq!(record.host.as_deref(), Some("www"));
        assert_eq!(record.points_to.as_deref(), Some("192.0.2.1"));
        assert_eq!(record.ttl, Some(Number::Literal(600)));
        assert_eq!(record.essential, None);
        let asked = prompter.transcript().join("\n");
        assert!(asked.contains("whole number"), "{asked}");
        assert!(asked.contains("not a valid essential value"), "{asked}");
    }

    #[test]
    fn cancelling_mid_record_stops_without_a_partial_record() {
        let prompter = ScriptedPrompter::new([
            Answer::Confirm(true),
            Answer::Select(0),
            Answer::text("www"),
            Answer::Cancel,
        ]);
        let mut template = template();
        let err = prompt_records(&prompter, &mut template).expect_err("cancelled");
        assert!(err.to_string().contains("cancelled"), "{err}");
        assert!(template.records.is_empty());
    }

    #[test]
    fn prompts_are_plain_text() {
        let mut answers = vec![Answer::Confirm(true), Answer::Select(0)];
        answers.extend(texts(&["", "ip", "", "", ""]));
        answers.push(Answer::Confirm(false));
        let prompter = ScriptedPrompter::new(answers);

        let mut template = template();
        prompt_records(&prompter, &mut template).expect("records");
        let asked = prompter.transcript().join("\n");
        assert!(asked.is_ascii(), "{asked}");
    }
}
