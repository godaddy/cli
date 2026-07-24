use super::types::OnboardingStatus;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgreementDecision {
    AlreadyComplete { org_id: String },
    CompletePending,
    AgreementsRequired,
    UnsupportedStatus,
}

pub fn decide(
    status: &OnboardingStatus,
    is_tty: bool,
    accept_flag: bool,
    prompt_accepted: Option<bool>,
) -> AgreementDecision {
    match status.status.as_str() {
        "ACTIVE" => AgreementDecision::AlreadyComplete {
            org_id: status.org_id.clone(),
        },
        "PENDING" if is_tty && prompt_accepted == Some(true) => AgreementDecision::CompletePending,
        "PENDING" if !is_tty && accept_flag => AgreementDecision::CompletePending,
        "PENDING" => AgreementDecision::AgreementsRequired,
        _ => AgreementDecision::UnsupportedStatus,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onboarding::OnboardingStatus;

    fn status(value: &str) -> OnboardingStatus {
        OnboardingStatus {
            org_id: "org-1".to_owned(),
            status: value.to_owned(),
        }
    }

    #[test]
    fn active_user_never_needs_acceptance() {
        assert_eq!(
            decide(&status("ACTIVE"), true, false, Some(false)),
            AgreementDecision::AlreadyComplete {
                org_id: "org-1".to_owned(),
            }
        );
    }

    #[test]
    fn pending_non_tty_requires_flag() {
        assert_eq!(
            decide(&status("PENDING"), false, false, None),
            AgreementDecision::AgreementsRequired
        );
    }

    #[test]
    fn pending_non_tty_flag_completes() {
        assert_eq!(
            decide(&status("PENDING"), false, true, None),
            AgreementDecision::CompletePending
        );
    }

    #[test]
    fn pending_tty_requires_successful_prompt() {
        assert_eq!(
            decide(&status("PENDING"), true, true, Some(true)),
            AgreementDecision::CompletePending
        );
        assert_eq!(
            decide(&status("PENDING"), true, true, Some(false)),
            AgreementDecision::AgreementsRequired
        );
    }

    #[test]
    fn unknown_status_is_not_accepted_or_prompted() {
        assert_eq!(
            decide(&status("SUSPENDED"), true, true, Some(true)),
            AgreementDecision::UnsupportedStatus
        );
    }
}
