use cli_engine::{CliCoreError, CommandContext};
use serde::Deserialize;
use serde_json::Value;

use crate::error::GddyError;
use crate::hosting::client::{ClientError, HostingClient};
use crate::http::api_url_for_env;
use crate::output_schema::output_schema;

output_schema!(HostingAppSummary {
    "id": "string";
    "name": "string";
    "status": "string";
});

output_schema!(HostingDeploymentSummary {
    "deploymentId": "string";
    "status": "string";
    "createdAt": "string";
    "updatedAt": "string";
    "gitHash": "string";
});

output_schema!(HostingSecretSummary {
    "name": "string";
    "systemManaged": "boolean";
});

output_schema!(HostingDomainSummary {
    "id": "string";
    "hostname": "string";
    "role": "string";
    "verificationStatus": "string";
    "domainType": "string";
    "certificateValidationCname": "string", optional;
    "anycastIp": "string", optional;
    "cdnStatus": "string", optional;
});

output_schema!(HostingSubscriptionList {
    "items": "array";
    "totalAvailableSlots": "number";
});

output_schema!(HostingSubscriptionAttachment {
    "subscriptionId": "string";
    "hostingProduct": "string";
    "attachState": "string";
    "appState": "string";
});

output_schema!(HostingLogEntry {
    "timestamp": "string";
    "level": "string";
    "source": "string";
    "message": "string";
});

output_schema!(HostingApplication {
    "id": "string";
    "name": "string";
    "appType": "string";
    "status": "string";
    "urls": "object";
    "createdAt": "string";
    "updatedAt": "string";
    "source": "string", optional;
    "sourceDetails": "object", optional;
});

output_schema!(HostingAppOperation {
    "operationId": "string";
    "status": "string";
    "app": "object", optional;
    "error": "object", optional;
    "createdAt": "string", optional;
    "links": "array";
});

output_schema!(HostingApplicationStatus {
    "status": "string";
    "variants": "array";
    "links": "array";
});

output_schema!(HostingSourceImport {
    "importId": "string";
    "importType": "string";
    "status": "string";
    "gitHash": "string", optional;
    "createdAt": "string";
    "links": "array";
});

output_schema!(HostingDomain {
    "domainId": "string";
    "hostname": "string";
    "role": "string";
    "verificationStatus": "string";
    "domainType": "string";
    "certificateValidationCname": "string", optional;
    "anycastIp": "string", optional;
    "cdnStatus": "string", optional;
    "links": "array";
});

output_schema!(HostingRuntime {
    "runtime": "string";
    "version": "string";
});

pub fn client_err(e: ClientError) -> CliCoreError {
    match e {
        ClientError::Http { status, body } => {
            GddyError::from_http(status, format_api_error_body(&body), "hosting").into_cli_error()
        }
        other => GddyError::from(other).into_cli_error(),
    }
}

/// Like [`client_err`], but overrides the fix hint.
pub fn client_err_with_fix(e: ClientError, fix: impl Into<String>) -> CliCoreError {
    match e {
        ClientError::Http { status, body } => {
            GddyError::from_http(status, format_api_error_body(&body), "hosting")
                .with_fix(fix)
                .into_cli_error()
        }
        other => GddyError::from(other).into_cli_error(),
    }
}

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    message: Option<String>,
    #[serde(default)]
    details: Vec<ApiErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct ApiErrorDetail {
    issue: Option<String>,
    description: Option<String>,
}

fn format_api_error_body(body: &str) -> String {
    let Ok(parsed) = serde_json::from_str::<ApiErrorBody>(body) else {
        return body.to_owned();
    };
    let message = parsed.message.unwrap_or_else(|| body.to_owned());
    let details: Vec<String> = parsed
        .details
        .iter()
        .filter_map(|d| d.issue.clone().or_else(|| d.description.clone()))
        .filter(|s| !s.is_empty() && *s != message)
        .collect();
    if details.is_empty() {
        message
    } else {
        format!("{message} ({})", details.join("; "))
    }
}

pub async fn make_client(
    ctx: &CommandContext,
    scopes: &[&str],
) -> cli_engine::Result<HostingClient> {
    let required: Vec<String> = scopes.iter().map(|s| (*s).to_owned()).collect();
    let token = ctx.credential_with_scopes(&required).await?.token;
    let base_url = api_url_for_env(&ctx.middleware.env)?;
    Ok(HostingClient::new(base_url, token))
}

#[derive(Debug, Clone, clap::Args)]
pub struct AppIdArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    pub app_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum HostingAppType {
    #[value(name = "NODEJS")]
    Nodejs,
}

impl HostingAppType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Nodejs => "NODEJS",
        }
    }
}

/// Extracts the `pageToken` value from `links[rel=next].href` in a paged response.
pub fn next_page_token(response: &Value) -> Option<String> {
    let links = response.get("links")?.as_array()?;
    for link in links {
        if link.get("rel").and_then(|v| v.as_str()) == Some("next") {
            let href = link.get("href").and_then(|v| v.as_str())?;
            return extract_query_param(href, "pageToken");
        }
    }
    None
}

fn extract_query_param(url: &str, param: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=')
            && k == param
        {
            return Some(v.to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::ValueEnum;
    use serde_json::json;

    #[test]
    fn hosting_app_type_parses_nodejs_case_insensitive() {
        assert_eq!(
            HostingAppType::from_str("NODEJS", true).expect("NODEJS"),
            HostingAppType::Nodejs
        );
        assert_eq!(
            HostingAppType::from_str("nodejs", true).expect("nodejs"),
            HostingAppType::Nodejs
        );
        assert_eq!(HostingAppType::Nodejs.as_str(), "NODEJS");
    }

    #[test]
    fn hosting_app_type_rejects_unknown() {
        assert!(HostingAppType::from_str("UNKNOWN", true).is_err());
        assert!(HostingAppType::from_str("", true).is_err());
    }

    #[test]
    fn next_page_token_extracts_from_links() {
        let response = json!({
            "items": [],
            "links": [
                { "rel": "self", "href": "https://api.godaddy.com/v1/hosting/apps?appType=NODEJS" },
                { "rel": "next", "href": "https://api.godaddy.com/v1/hosting/apps?appType=NODEJS&pageToken=tok-2&pageSize=10" }
            ]
        });
        assert_eq!(next_page_token(&response).as_deref(), Some("tok-2"));
    }

    #[test]
    fn next_page_token_returns_none_when_no_next_link() {
        let response = json!({
            "items": [],
            "links": [
                { "rel": "self", "href": "https://api.godaddy.com/v1/hosting/apps?appType=NODEJS" }
            ]
        });
        assert!(next_page_token(&response).is_none());
    }

    #[test]
    fn next_page_token_returns_none_when_no_links() {
        let response = json!({ "items": [] });
        assert!(next_page_token(&response).is_none());
    }

    #[test]
    fn format_api_error_body_prefers_issue_over_repeated_description() {
        let body = r#"{
            "name": "UnprocessableEntity",
            "message": "This app must be attached to a Web Hosting plan to publish.",
            "details": [{
                "issue": "WH_PLAN_REQUIRED",
                "description": "This app must be attached to a Web Hosting plan to publish."
            }]
        }"#;
        assert_eq!(
            format_api_error_body(body),
            "This app must be attached to a Web Hosting plan to publish. (WH_PLAN_REQUIRED)"
        );
    }

    #[test]
    fn format_api_error_body_falls_back_to_description_without_issue() {
        let body = r#"{"message": "bad request", "details": [{"description": "missing field"}]}"#;
        assert_eq!(format_api_error_body(body), "bad request (missing field)");
    }

    #[test]
    fn format_api_error_body_passes_through_unparseable_bodies() {
        assert_eq!(format_api_error_body("not json"), "not json");
    }

    #[test]
    fn client_err_with_fix_overrides_the_default_fix() {
        let err = client_err_with_fix(
            ClientError::Http {
                status: 422,
                body: r#"{"message":"This app must be attached to a Web Hosting plan to publish.","details":[{"issue":"WH_PLAN_REQUIRED"}]}"#.to_owned(),
            },
            "Run: gddy hosting subscription list",
        );
        let envelope = cli_engine::build_error_envelope(&err, "hosting");
        assert_eq!(
            envelope.error.as_ref().map(|e| e.message.as_str()),
            Some(
                "HTTP error 422: This app must be attached to a Web Hosting plan to publish. (WH_PLAN_REQUIRED)"
            )
        );
        assert_eq!(
            envelope.fix.as_deref(),
            Some("Run: gddy hosting subscription list")
        );
    }

    #[test]
    fn client_err_maps_http_status_to_hosting_system() {
        let err = client_err(ClientError::Http {
            status: 404,
            body: r#"{"message":"app not found"}"#.to_owned(),
        });
        let envelope = cli_engine::build_error_envelope(&err, "hosting");
        assert_eq!(
            envelope.error.as_ref().map(|e| e.message.as_str()),
            Some("HTTP error 404: app not found")
        );
        assert!(
            envelope
                .fix
                .as_deref()
                .is_some_and(|f| f.contains("hosting app list")),
            "{envelope:?}"
        );
    }
}
