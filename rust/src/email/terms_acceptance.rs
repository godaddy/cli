//! Microsoft Customer Agreement (MCA) acceptance gate for email eligibility.
//!
//! When the eligibility API returns a `requirements` entry with
//! `type: "MICROSOFT_MCA"`, the CLI opens the MCA page in the browser and
//! polls the productivity API attestation endpoint until the customer
//! clicks Accept. The caller then retries the eligibility check automatically.
//!
//! ## Requirement entry (in the 200 eligibility response)
//!
//! ```json
//! {
//!   "type": "MICROSOFT_MCA",
//!   "title": "Microsoft Customer Agreement",
//!   "reference": "https://cdn.partner.microsoft.com/mca/?attestationid=<id>"
//! }
//! ```
//!
//! The attestation ID is the `attestationid` query parameter of `reference`.
//!
//! ## Attestation status endpoint (productivity API)
//!
//! `GET /v1/email/m365-attestation-status/<id>`
//!
//! Returns `{ "status": "Pending" | "Accepted", ... }`.
//! Polling continues until `status == "Accepted"`.

use std::time::{Duration, Instant};

use cli_engine::StreamSender;
use serde_json::{Value, json};
use tracing::debug;

use crate::email::client::{ClientError, EmailClient};
use crate::error::GddyError;

const POLL_INTERVAL: Duration = Duration::from_secs(3);
const TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes

/// The acceptance URL and attestation ID for a single MCA gate.
#[derive(Debug)]
pub(super) struct TermsRequirement {
    /// The Microsoft MCA page to open in the browser.
    pub(super) acceptance_url: String,
    /// Derived from the `attestationid` query param of `acceptance_url`.
    /// Used to poll Microsoft's attestation status endpoint.
    pub(super) attestation_id: String,
}

impl TermsRequirement {
    /// Builds a [`TermsRequirement`] from the MCA page URL, extracting the
    /// attestation ID from the `attestationid` query param. Returns `None`
    /// when the URL does not contain the required param.
    pub(super) fn from_url(url: String) -> Option<Self> {
        let attestation_id = attestation_id_from_url(&url)?;
        Some(Self {
            acceptance_url: url,
            attestation_id,
        })
    }
}

/// Scans a 200 eligibility response for a `MICROSOFT_MCA` requirement and
/// returns the corresponding [`TermsRequirement`].
///
/// Returns `None` when no such requirement is present (common case: no T&C
/// gate) or when the URL is missing the `attestationid` query param.
pub(super) fn extract_mca_requirement(data: &Value) -> Option<TermsRequirement> {
    let requirements = data.get("requirements")?.as_array()?;
    requirements.iter().find_map(|req| {
        if req.get("type").and_then(Value::as_str) == Some("MICROSOFT_MCA") {
            let url = req.get("reference").and_then(Value::as_str)?.to_owned();
            let attestation_id = attestation_id_from_url(&url)?;
            Some(TermsRequirement {
                acceptance_url: url,
                attestation_id,
            })
        } else {
            None
        }
    })
}

/// Extracts the `attestationid` query parameter value from a Microsoft MCA URL.
/// e.g. `https://cdn.partner.microsoft.com/mca/?attestationid=abc-123` → `"abc-123"`.
fn attestation_id_from_url(url: &str) -> Option<String> {
    url.split_once('?')?.1.split('&').find_map(|param| {
        let (key, value) = param.split_once('=')?;
        (key == "attestationid").then(|| value.to_owned())
    })
}

/// Opens the Microsoft MCA page in the browser, then polls the Microsoft
/// Partner Center attestation API until the customer clicks Accept (or the
/// 5-minute timeout expires).
///
/// Emits three non-terminal streaming events:
/// - `terms_acceptance_required` — immediately; includes the URL and whether
///   the browser opened successfully.
/// - `terms_acceptance_polling` — on each poll; useful for agent progress.
/// - `terms_acceptance_completed` — once `status == "Accepted"`.
///
/// Returns `Err` on timeout or non-retryable API failure. The caller is
/// responsible for emitting the terminal error event before returning.
pub(super) async fn wait_for_acceptance(
    requirement: &TermsRequirement,
    client: &EmailClient,
    sender: &StreamSender,
) -> cli_engine::Result<()> {
    let browser_opened = open::that(&requirement.acceptance_url).is_ok();
    sender
        .send(json!({
            "type": "terms_acceptance_required",
            "url": requirement.acceptance_url,
            "attestationId": requirement.attestation_id,
            "browserOpened": browser_opened,
        }))
        .await;

    let started = Instant::now();
    let mut attempt: u32 = 0;

    loop {
        if started.elapsed() >= TIMEOUT {
            return Err(GddyError::validation(format!(
                "Timed out waiting for Microsoft Customer Agreement acceptance \
                 after {} seconds.",
                TIMEOUT.as_secs()
            ))
            .with_fix("Accept the agreement at the URL shown above, then re-run the command.")
            .into_cli_error());
        }

        tokio::time::sleep(POLL_INTERVAL).await;
        attempt += 1;

        sender
            .send(json!({
                "type": "terms_acceptance_polling",
                "attempt": attempt,
            }))
            .await;

        match poll_attestation_status(client, &requirement.attestation_id).await {
            Ok(AttestationStatus::Accepted) => {
                sender
                    .send(json!({
                        "type": "terms_acceptance_completed",
                        "attestationId": requirement.attestation_id,
                        "attempts": attempt,
                    }))
                    .await;
                return Ok(());
            }
            Ok(AttestationStatus::Pending) => {
                debug!(attempt, "MCA attestation still pending");
            }
            Err(e) => {
                // Transient errors during polling don't fail the flow; we
                // keep polling until TIMEOUT so a brief network hiccup
                // doesn't abort a customer mid-acceptance.
                debug!(error = %e, "transient error polling attestation status");
            }
        }
    }
}

#[derive(Debug, PartialEq)]
enum AttestationStatus {
    Pending,
    Accepted,
}

async fn poll_attestation_status(
    client: &EmailClient,
    attestation_id: &str,
) -> Result<AttestationStatus, ClientError> {
    let body = client.get_attestation_status(attestation_id).await?;
    let status = body
        .get("status")
        .and_then(|s| s.as_str())
        .unwrap_or("Pending");
    Ok(if status == "Accepted" {
        AttestationStatus::Accepted
    } else {
        AttestationStatus::Pending
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::email::client::EmailClient;

    #[test]
    fn extract_returns_none_when_no_requirements() {
        assert!(extract_mca_requirement(&json!({ "isEligible": true })).is_none());
    }

    #[test]
    fn extract_returns_none_for_unrelated_requirement_type() {
        let data = json!({
            "requirements": [{ "type": "FREETRIAL_AUTORENEW" }]
        });
        assert!(extract_mca_requirement(&data).is_none());
    }

    #[test]
    fn extract_returns_none_when_reference_missing() {
        let data = json!({
            "requirements": [{ "type": "MICROSOFT_MCA", "title": "MCA" }]
        });
        assert!(extract_mca_requirement(&data).is_none());
    }

    #[test]
    fn extract_returns_none_when_attestationid_param_absent() {
        let data = json!({
            "requirements": [{
                "type": "MICROSOFT_MCA",
                "reference": "https://cdn.partner.microsoft.com/mca/"
            }]
        });
        assert!(extract_mca_requirement(&data).is_none());
    }

    #[test]
    fn extract_parses_full_mca_requirement() {
        let data = json!({
            "requirements": [{
                "type": "MICROSOFT_MCA",
                "title": "Microsoft Customer Agreement",
                "reference": "https://cdn.partner.microsoft.com/mca/?attestationid=1860fc65-9382-4629-9659-e26c7d20caf6"
            }]
        });
        let req = extract_mca_requirement(&data).expect("should find MCA requirement");
        assert_eq!(
            req.acceptance_url,
            "https://cdn.partner.microsoft.com/mca/?attestationid=1860fc65-9382-4629-9659-e26c7d20caf6"
        );
        assert_eq!(req.attestation_id, "1860fc65-9382-4629-9659-e26c7d20caf6");
    }

    #[test]
    fn extract_finds_mca_among_multiple_requirements() {
        let data = json!({
            "requirements": [
                { "type": "FREETRIAL_AUTORENEW" },
                {
                    "type": "MICROSOFT_MCA",
                    "title": "Microsoft Customer Agreement",
                    "reference": "https://cdn.partner.microsoft.com/mca/?attestationid=abc-123"
                }
            ]
        });
        let req = extract_mca_requirement(&data).expect("should find MCA");
        assert_eq!(req.attestation_id, "abc-123");
    }

    #[test]
    fn attestation_id_from_url_parses_single_param() {
        assert_eq!(
            attestation_id_from_url(
                "https://cdn.partner.microsoft.com/mca/?attestationid=ef131860-35ab-48a5-b33b-b5d7a908d2d5"
            ),
            Some("ef131860-35ab-48a5-b33b-b5d7a908d2d5".to_owned())
        );
    }

    #[test]
    fn attestation_id_from_url_parses_when_other_params_present() {
        assert_eq!(
            attestation_id_from_url(
                "https://example.com/mca/?foo=bar&attestationid=abc-123&baz=qux"
            ),
            Some("abc-123".to_owned())
        );
    }

    #[test]
    fn attestation_id_from_url_returns_none_without_query() {
        assert!(attestation_id_from_url("https://example.com/mca/").is_none());
    }

    #[tokio::test]
    async fn poll_attestation_status_returns_accepted() {
        let server = httpmock::prelude::MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(httpmock::prelude::GET)
                    .path("/v1/email/m365-attestation-status/1860fc65-9382-4629-9659-e26c7d20caf6");
                then.status(200).json_body(json!({
                    "attestationId": "1860fc65-9382-4629-9659-e26c7d20caf6",
                    "confirmationId": "7254bba9-deb6-4e09-941f-b006385abe09",
                    "status": "Accepted"
                }));
            })
            .await;

        let client = EmailClient::new(server.base_url(), "test-token");
        let status = poll_attestation_status(&client, "1860fc65-9382-4629-9659-e26c7d20caf6")
            .await
            .expect("should succeed");

        mock.assert_async().await;
        assert_eq!(status, AttestationStatus::Accepted);
    }

    #[tokio::test]
    async fn poll_attestation_status_returns_pending() {
        let server = httpmock::prelude::MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::prelude::GET)
                    .path("/v1/email/m365-attestation-status/id-1");
                then.status(200).json_body(json!({
                    "attestationId": "id-1",
                    "status": "Pending"
                }));
            })
            .await;

        let client = EmailClient::new(server.base_url(), "test-token");
        let status = poll_attestation_status(&client, "id-1")
            .await
            .expect("should succeed");
        assert_eq!(status, AttestationStatus::Pending);
    }
}
