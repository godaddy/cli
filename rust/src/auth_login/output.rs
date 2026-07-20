use std::borrow::Cow;
use std::error::Error;
use std::fmt::{Display, Formatter};

use cli_engine::{DetailedError, Envelope, build_detailed_error_envelope, render_format};
use serde_json::{Map, Value};

use crate::next_action::next_action;

pub(crate) const ONBOARDING_WARNING: &str =
    "Authentication succeeded, but onboarding could not be checked.";

#[derive(Debug)]
pub(crate) struct AgreementsRequiredError;

impl Display for AgreementsRequiredError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("GoDaddy Developer agreements must be accepted to finish onboarding.")
    }
}

impl Error for AgreementsRequiredError {}

impl DetailedError for AgreementsRequiredError {
    fn error_code(&self) -> Cow<'static, str> {
        Cow::Borrowed("AGREEMENTS_REQUIRED")
    }

    fn error_system(&self) -> Option<Cow<'static, str>> {
        Some(Cow::Borrowed("auth"))
    }

    fn error_request_id(&self) -> Option<Cow<'static, str>> {
        None
    }
}

pub(crate) fn apply_complete(envelope: &mut Envelope, org_id: impl Into<String>) {
    let data = object_data(envelope);
    data.insert("org_id".to_owned(), Value::String(org_id.into()));
    data.insert(
        "onboarding".to_owned(),
        Value::String("complete".to_owned()),
    );
}

pub(crate) fn apply_failed(envelope: &mut Envelope) {
    object_data(envelope).insert("onboarding".to_owned(), Value::String("failed".to_owned()));
    if !envelope
        .warnings
        .iter()
        .any(|warning| warning == ONBOARDING_WARNING)
    {
        envelope.warnings.push(ONBOARDING_WARNING.to_owned());
    }
}

pub(crate) fn agreements_required_envelope() -> Envelope {
    build_detailed_error_envelope(&AgreementsRequiredError, "auth").with_next_actions(vec![
        next_action(
            "auth login --accept-agreements",
            "Accept GoDaddy Developer agreements non-interactively",
        ),
    ])
}

pub(crate) fn render(format: &str, envelope: &Envelope) -> cli_engine::Result<String> {
    render_format(format, envelope)
}

fn object_data(envelope: &mut Envelope) -> &mut Map<String, Value> {
    if envelope
        .data
        .as_mut()
        .and_then(Value::as_object_mut)
        .is_none()
    {
        envelope.data = Some(Value::Object(Map::new()));
    }

    envelope
        .data
        .as_mut()
        .and_then(Value::as_object_mut)
        .expect("envelope data was replaced with an object")
}

#[cfg(test)]
mod tests {
    use cli_engine::{Envelope, NextAction};
    use serde_json::json;

    fn login_envelope() -> Envelope {
        Envelope::success(
            json!({
                "provider": "godaddy",
                "env": "ote",
                "identity": "customer",
                "expires_at": "2026-07-20T00:00:00Z",
                "scopes": []
            }),
            "auth",
        )
        .with_next_actions(vec![NextAction::new("gddy auth status", "Check login")])
    }

    #[test]
    fn completion_preserves_login_data_and_envelope_context() {
        let mut envelope = login_envelope();
        let metadata = envelope.metadata.clone();
        let next_actions = envelope.next_actions.clone();

        super::apply_complete(&mut envelope, "org-123");

        let data = envelope.data.expect("completion data");
        assert_eq!(data["provider"], "godaddy");
        assert_eq!(data["env"], "ote");
        assert_eq!(data["identity"], "customer");
        assert_eq!(data["expires_at"], "2026-07-20T00:00:00Z");
        assert_eq!(data["scopes"], json!([]));
        assert_eq!(data["org_id"], "org-123");
        assert_eq!(data["onboarding"], "complete");
        assert_eq!(envelope.metadata, metadata);
        assert_eq!(envelope.next_actions, next_actions);
    }

    #[test]
    fn completion_replaces_non_object_data_with_onboarding_data() {
        let mut envelope = Envelope::success("unexpected", "auth");

        super::apply_complete(&mut envelope, "org-123");

        assert_eq!(
            envelope.data,
            Some(json!({
                "org_id": "org-123",
                "onboarding": "complete"
            }))
        );
    }

    #[test]
    fn failure_marks_onboarding_and_adds_stable_warning() {
        let mut envelope = login_envelope();

        super::apply_failed(&mut envelope);

        assert_eq!(
            envelope.data.as_ref().expect("failure data")["onboarding"],
            "failed"
        );
        assert_eq!(
            envelope.warnings,
            vec![super::ONBOARDING_WARNING.to_owned()]
        );
    }

    #[test]
    fn repeated_failure_does_not_duplicate_warning() {
        let mut envelope = login_envelope();

        super::apply_failed(&mut envelope);
        super::apply_failed(&mut envelope);

        assert_eq!(
            envelope
                .warnings
                .iter()
                .filter(|warning| warning.as_str() == super::ONBOARDING_WARNING)
                .count(),
            1
        );
    }

    #[test]
    fn agreements_required_is_a_structured_auth_error_with_next_action() {
        let envelope = super::agreements_required_envelope();
        let error = envelope.error.expect("structured error");

        assert_eq!(error.code, "AGREEMENTS_REQUIRED");
        assert_eq!(error.system, "auth");
        assert_eq!(envelope.next_actions.len(), 1);
        assert_eq!(
            envelope.next_actions[0].command,
            "gddy auth login --accept-agreements"
        );
        assert_eq!(
            envelope.next_actions[0].description,
            "Accept GoDaddy Developer agreements non-interactively"
        );
    }

    #[test]
    fn native_envelope_renders_in_all_supported_formats() -> cli_engine::Result<()> {
        let envelope = login_envelope();

        for format in ["json", "human", "toon"] {
            let rendered = super::render(format, &envelope)?;
            assert!(!rendered.is_empty(), "{format} rendering was empty");
        }

        Ok(())
    }
}
