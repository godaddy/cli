use serde::Deserialize;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OnboardingStatus {
    pub org_id: String,
    pub status: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliOnboardingResult {
    pub organization_id: String,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ApiEnvelope<T> {
    pub success: bool,
    pub data: T,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StatusData {
    pub id: String,
    pub status: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CliData {
    pub organization_id: String,
    pub status: String,
}
