//! Interactive prompts for `template init`. Every prompt reads from a
//! `BufRead` and writes to a `Write`, so tests can drive it with a `Cursor`.

use std::io::{BufRead, Write};

use super::parsers::{self, Field, Need};
use super::types::{Record, RecordKind, Template};
use crate::error::GddyError;

const SYSTEM: &str = "domain-connect";

fn io_error(error: std::io::Error) -> cli_engine::CliCoreError {
    GddyError::unexpected(format!("could not read your answer: {error}"))
        .with_system(SYSTEM)
        .into_cli_error()
}

fn say(writer: &mut impl Write, text: &str) -> cli_engine::Result<()> {
    writeln!(writer, "{text}").map_err(io_error)
}

/// Asks one question and returns the trimmed answer. Fails when input ends,
/// so a run with no one to answer stops instead of looping.
fn ask(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    prompt: &str,
) -> cli_engine::Result<String> {
    write!(writer, "{prompt}: ").map_err(io_error)?;
    writer.flush().map_err(io_error)?;
    let mut line = String::new();
    if reader.read_line(&mut line).map_err(io_error)? == 0 {
        return Err(GddyError::validation(
            "Input ended before the template was complete; no file was written.",
        )
        .with_fix(
            "Run this command in a terminal to answer the prompts, or pass --no-interactive \
             with --provider-id, --provider-name, --service-id and --service-name.",
        )
        .with_system(SYSTEM)
        .into_cli_error());
    }
    Ok(line.trim().to_owned())
}

/// Keeps asking until `accept` takes the answer, showing why it was refused.
fn ask_until<T>(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    prompt: &str,
    mut accept: impl FnMut(String) -> Result<T, GddyError>,
) -> cli_engine::Result<T> {
    loop {
        match accept(ask(reader, writer, prompt)?) {
            Ok(value) => return Ok(value),
            Err(problem) => say(writer, &format!("  {problem}"))?,
        }
    }
}

fn required(answer: String) -> Result<String, GddyError> {
    if answer.is_empty() {
        Err(GddyError::validation("A value is required."))
    } else {
        Ok(answer)
    }
}

/// Collects a whole template: the provider and service, then any records.
pub(super) fn prompt_template(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
) -> cli_engine::Result<Template> {
    say(writer, "Create a Domain Connect template")?;
    say(writer, "")?;
    let provider_id = ask_until(
        reader,
        writer,
        "Provider ID (your domain name, such as acme.com)",
        |answer| super::init::check_provider_id(&answer).map(|()| answer),
    )?;
    let provider_name = ask_until(reader, writer, "Provider name", required)?;
    let service_id = ask_until(
        reader,
        writer,
        "Service ID (such as managed-wordpress)",
        |answer| super::init::check_service_id(&answer).map(|()| answer),
    )?;
    let service_name = ask_until(reader, writer, "Service name", required)?;
    let description = ask(reader, writer, "Description (optional)")?;

    let mut template = Template::from_basic_inputs(
        provider_id,
        provider_name,
        service_id,
        service_name,
        (!description.is_empty()).then_some(description),
    );

    say(writer, "")?;
    say(writer, "DNS records")?;
    while confirm(reader, writer, "Add a record?")? {
        let kind = prompt_kind(reader, writer)?;
        template.records.push(prompt_record(reader, writer, kind)?);
        say(writer, &format!("Added {} record.", kind.name()))?;
    }
    Ok(template)
}

fn confirm(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    prompt: &str,
) -> cli_engine::Result<bool> {
    ask_until(
        reader,
        writer,
        &format!("{prompt} (y/N)"),
        |answer| match answer.to_ascii_lowercase().as_str() {
            "y" | "yes" => Ok(true),
            "" | "n" | "no" => Ok(false),
            _ => Err(GddyError::validation("Answer y or n.")),
        },
    )
}

fn prompt_kind(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
) -> cli_engine::Result<RecordKind> {
    say(writer, "Record type:")?;
    for (index, kind) in RecordKind::ALL.iter().enumerate() {
        say(
            writer,
            &format!(
                "  {:>2}. {} ({})",
                index + 1,
                kind.name(),
                kind.description()
            ),
        )?;
    }
    let count = RecordKind::ALL.len();
    ask_until(reader, writer, &format!("Select (1-{count})"), |answer| {
        answer
            .parse::<usize>()
            .ok()
            .and_then(|number| number.checked_sub(1))
            .and_then(|index| RecordKind::ALL.get(index).copied())
            .ok_or_else(|| GddyError::validation(format!("Enter a number from 1 to {count}.")))
    })
}

fn prompt_record(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    kind: RecordKind,
) -> cli_engine::Result<Record> {
    let mut record = Record::new(kind);
    for field in parsers::fields(kind) {
        let prompt = field_prompt(field);
        ask_until(reader, writer, &prompt, |answer| {
            let value = match (answer.is_empty(), &field.need) {
                (false, _) => answer,
                (true, Need::Default(default)) => (*default).to_owned(),
                (true, Need::Optional) => return Ok(()),
                (true, Need::Required) => {
                    return Err(GddyError::validation("A value is required."));
                }
            };
            parsers::set(&mut record, kind, field.attr, value)
        })?;
    }
    Ok(record)
}

fn field_prompt(field: &Field) -> String {
    match &field.need {
        Need::Required => field.label.to_owned(),
        Need::Optional => format!("{} (optional)", field.label),
        Need::Default(default) => format!("{} [{default}]", field.label),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::domain_connect::template::types::Essential;

    fn run(input: &str) -> (cli_engine::Result<Template>, String) {
        let mut reader = Cursor::new(input.as_bytes().to_vec());
        let mut output = Vec::new();
        let result = prompt_template(&mut reader, &mut output);
        (result, String::from_utf8(output).expect("utf8 prompts"))
    }

    #[test]
    fn a_full_session_builds_the_template() {
        // Provider, service, description, then one MX record taking the
        // defaults for host, priority and TTL.
        let (result, _) = run(concat!(
            "acme.com\nAcme Corp\nemail\nAcme Email\nMail setup\n",
            "y\n4\n\nmail.acme.com\n\n\nmail\nOnApply\n",
            "n\n",
        ));

        let template = result.expect("template");
        assert_eq!(template.provider_id, "acme.com");
        assert_eq!(template.service_id, "email");
        assert_eq!(template.description.as_deref(), Some("Mail setup"));
        assert_eq!(template.records.len(), 1);
        let record = &template.records[0];
        assert_eq!(record.record_type, "MX");
        assert_eq!(record.host.as_deref(), Some("@"));
        assert_eq!(record.points_to.as_deref(), Some("mail.acme.com"));
        assert_eq!(record.priority, Some(10));
        assert_eq!(record.ttl, Some(3600));
        assert_eq!(record.group_id.as_deref(), Some("mail"));
        assert_eq!(record.essential, Some(Essential::OnApply));
    }

    #[test]
    fn an_invalid_answer_is_explained_and_asked_again() {
        let (result, prompts) = run("acme\nacme.com\nAcme\nbad id\nemail\nEmail\n\nn\n");

        let template = result.expect("template");
        assert_eq!(template.provider_id, "acme.com");
        assert_eq!(template.service_id, "email");
        assert_eq!(template.description, None);
        assert!(prompts.contains("domain name"), "{prompts}");
        assert!(prompts.contains("letters, digits"), "{prompts}");
    }

    #[test]
    fn a_bad_record_value_is_asked_again_instead_of_ending_the_session() {
        let (result, prompts) = run(concat!(
            "acme.com\nAcme\nweb\nWeb\n\n",
            "y\n99\n1\nwww\n192.0.2.1\nsoon\n600\n\nNo\n\n",
            "n\n",
        ));

        let template = result.expect("template");
        let record = &template.records[0];
        assert_eq!(record.record_type, "A");
        assert_eq!(record.host.as_deref(), Some("www"));
        assert_eq!(record.points_to.as_deref(), Some("192.0.2.1"));
        assert_eq!(record.ttl, Some(600));
        assert_eq!(record.essential, None);
        assert!(prompts.contains("Enter a number from 1 to 12"), "{prompts}");
        assert!(prompts.contains("whole number"), "{prompts}");
        assert!(prompts.contains("not a valid essential value"), "{prompts}");
    }

    #[test]
    fn input_ending_early_fails_with_a_fix_instead_of_a_partial_template() {
        let (result, _) = run("acme.com\n");

        let err = result.expect_err("input ended");
        assert!(err.to_string().contains("Input ended"), "{err}");
        let fix = cli_engine::build_error_envelope(&err, SYSTEM)
            .fix
            .unwrap_or_default();
        assert!(fix.contains("--no-interactive"), "{fix}");
    }

    #[test]
    fn prompts_are_plain_text() {
        let (_, prompts) = run("acme.com\nAcme\nweb\nWeb\n\ny\n1\n\nip\n\n\n\nn\n");
        assert!(prompts.is_ascii(), "{prompts}");
    }
}
