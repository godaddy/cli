//! Microsoft Customer Agreement (MCA) requirement detection for email eligibility.
//!
//! When the eligibility API returns a `requirements` entry with
//! `type: "MICROSOFT_MCA"`, the `create` command opens the MCA acceptance URL
//! in the browser and asks the user to re-run after accepting.
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

use std::time::Duration;

use cli_engine::CliCoreError;
use email_client::types::EligibleAccount;

use crate::email::client::EmailClient;
use crate::error::GddyError;

const POLL_INTERVAL: Duration = Duration::from_secs(5);
const TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes

/// The acceptance URL for a single MCA gate.
#[derive(Debug)]
pub(super) struct TermsRequirement {
    /// The Microsoft MCA page to open in the browser.
    pub(super) acceptance_url: String,
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
            // Require a parseable attestationid so we surface URLs that lack
            // the parameter early, rather than sending the user to a broken page.
            attestation_id_from_url(&url)?;
            Some(TermsRequirement {
                acceptance_url: url,
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

/// Polls `check_eligibility` until the `MICROSOFT_MCA` requirement disappears
/// (the user accepted in the browser), or until [`TIMEOUT`] elapses.
///
/// Called after the acceptance URL has been opened in the browser.
/// Returns `Ok(())` when the requirement is gone; errors on timeout.
pub(super) async fn wait_for_mca_acceptance(
    client: &EmailClient,
    email: &str,
) -> Result<(), CliCoreError> {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;

        if let Ok(eligibility) = client.check_eligibility(email).await {
            let still_pending = eligibility
                .eligible_accounts
                .iter()
                .any(|acct| extract_mca_requirement(acct).is_some());
            if !still_pending {
                return Ok(());
            }
        }

        if tokio::time::Instant::now() >= deadline {
            break;
        }
    }

    Err(
        GddyError::validation("Timed out waiting for Microsoft Customer Agreement acceptance.")
            .with_fix(
                "If you have already accepted the agreement, re-run this command. \
             Otherwise, re-run and accept the agreement when prompted.",
            )
            .into_cli_error(),
    )
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
        assert!(req.acceptance_url.contains("abc-123"));
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
}
