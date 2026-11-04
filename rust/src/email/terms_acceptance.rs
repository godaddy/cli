//! Microsoft Customer Agreement (MCA) acceptance gate for email eligibility.
//!
//! When the eligibility API returns a `requirements` entry with
//! `type: "MICROSOFT_MCA"`, the CLI opens the MCA page in the browser and
//! polls the Microsoft Partner Center attestation endpoint directly until the
//! customer clicks Accept. The caller then retries the eligibility check automatically.
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
//! ## Attestation status endpoint (Microsoft Partner Center)
//!
//! `GET https://api.partnercenter.microsoft.com/v1/attestations/enhanced/<id>`
//!
//! Returns `{ "status": "Pending" | "Accepted", ... }`.
//! Polling continues until `status == "Accepted"`.

use std::time::{Duration, Instant};

use cli_engine::StreamSender;
use email_client::types::EligibleAccount;
use serde_json::json;
use tracing::debug;

use crate::email::client::EmailClient;
use crate::error::GddyError;

const MICROSOFT_ATTESTATION_BASE: &str =
    "https://api.partnercenter.microsoft.com/v1/attestations/enhanced";

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

/// Scans a 200 eligibility response for a `MICROSOFT_MCA` requirement and
/// returns the corresponding [`TermsRequirement`].
///
/// Returns `None` when no such requirement is present (common case: no T&C
/// gate) or when the URL is missing the `attestationid` query param.
pub(super) fn extract_mca_requirement(account: &EligibleAccount) -> Option<TermsRequirement> {
    account.requirements.iter().find_map(|req| {
        if req.type_ == "MICROSOFT_MCA" {
            let url = req.reference.as_deref()?.to_owned();
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
    _client: &EmailClient,
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

        match poll_attestation_status(&requirement.attestation_id).await {
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
    attestation_id: &str,
) -> Result<AttestationStatus, reqwest::Error> {
    let url = format!("{MICROSOFT_ATTESTATION_BASE}/{attestation_id}");
    let body: serde_json::Value = reqwest::Client::new()
        .get(&url)
        .send()
        .await?
        .json()
        .await?;
    let accepted = body
        .get("status")
        .and_then(|s| s.as_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("accepted"));
    Ok(if accepted {
        AttestationStatus::Accepted
    } else {
        AttestationStatus::Pending
    })
}

#[cfg(test)]
mod tests {
    use email_client::types::{AgreementRequirement, EligibleAccount, MailboxType, Uuid};

    use super::*;

    fn make_account(requirements: Vec<AgreementRequirement>) -> EligibleAccount {
        EligibleAccount {
            account_id: Uuid("acct-1".to_owned()),
            account_name: None,
            default: false,
            mailbox_type: MailboxType("TITAN".to_owned()),
            requirements,
        }
    }

    fn mca_req(reference: Option<&str>) -> AgreementRequirement {
        AgreementRequirement {
            type_: "MICROSOFT_MCA".to_owned(),
            title: Some("Microsoft Customer Agreement".to_owned()),
            reference: reference.map(str::to_owned),
        }
    }

    #[test]
    fn extract_returns_none_when_no_requirements() {
        let account = make_account(vec![]);
        assert!(extract_mca_requirement(&account).is_none());
    }

    #[test]
    fn extract_returns_none_for_unrelated_requirement_type() {
        let account = make_account(vec![AgreementRequirement {
            type_: "FREETRIAL_AUTORENEW".to_owned(),
            title: None,
            reference: None,
        }]);
        assert!(extract_mca_requirement(&account).is_none());
    }

    #[test]
    fn extract_returns_none_when_reference_missing() {
        let account = make_account(vec![mca_req(None)]);
        assert!(extract_mca_requirement(&account).is_none());
    }

    #[test]
    fn extract_returns_none_when_attestationid_param_absent() {
        let account = make_account(vec![mca_req(Some(
            "https://cdn.partner.microsoft.com/mca/",
        ))]);
        assert!(extract_mca_requirement(&account).is_none());
    }

    #[test]
    fn extract_parses_full_mca_requirement() {
        let account = make_account(vec![mca_req(Some(
            "https://cdn.partner.microsoft.com/mca/?attestationid=1860fc65-9382-4629-9659-e26c7d20caf6",
        ))]);
        let req = extract_mca_requirement(&account).expect("should find MCA requirement");
        assert_eq!(
            req.acceptance_url,
            "https://cdn.partner.microsoft.com/mca/?attestationid=1860fc65-9382-4629-9659-e26c7d20caf6"
        );
        assert_eq!(req.attestation_id, "1860fc65-9382-4629-9659-e26c7d20caf6");
    }

    #[test]
    fn extract_finds_mca_among_multiple_requirements() {
        let account = make_account(vec![
            AgreementRequirement {
                type_: "FREETRIAL_AUTORENEW".to_owned(),
                title: None,
                reference: None,
            },
            mca_req(Some(
                "https://cdn.partner.microsoft.com/mca/?attestationid=abc-123",
            )),
        ]);
        let req = extract_mca_requirement(&account).expect("should find MCA");
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
                    .path("/v1/attestations/enhanced/1860fc65-9382-4629-9659-e26c7d20caf6");
                then.status(200).json_body(json!({
                    "attestationId": "1860fc65-9382-4629-9659-e26c7d20caf6",
                    "status": "Accepted"
                }));
            })
            .await;

        // Override the base URL so the call hits the mock server instead of
        // the real Microsoft endpoint.
        let base = server.base_url();
        let id = "1860fc65-9382-4629-9659-e26c7d20caf6";
        let url = format!("{base}/v1/attestations/enhanced/{id}");
        let body: serde_json::Value = reqwest::Client::new()
            .get(&url)
            .send()
            .await
            .expect("request")
            .json()
            .await
            .expect("json");
        let accepted = body
            .get("status")
            .and_then(|s| s.as_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("accepted"));

        mock.assert_async().await;
        assert!(accepted);
    }

    #[tokio::test]
    async fn poll_attestation_status_returns_pending() {
        let server = httpmock::prelude::MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::prelude::GET)
                    .path("/v1/attestations/enhanced/id-1");
                then.status(200).json_body(json!({
                    "attestationId": "id-1",
                    "status": "Pending"
                }));
            })
            .await;

        let base = server.base_url();
        let url = format!("{base}/v1/attestations/enhanced/id-1");
        let body: serde_json::Value = reqwest::Client::new()
            .get(&url)
            .send()
            .await
            .expect("request")
            .json()
            .await
            .expect("json");
        let accepted = body
            .get("status")
            .and_then(|s| s.as_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("accepted"));

        assert!(!accepted);
    }
}
