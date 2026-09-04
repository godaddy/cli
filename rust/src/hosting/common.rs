use cli_engine::{CliCoreError, CommandContext};
use serde_json::Value;

use crate::application::client::api_url_for_env;
use crate::hosting::client::{ClientError, HostingClient};
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
});

output_schema!(HostingSubscriptionSummary {
    "subscriptionId": "string";
    "hostingProduct": "string";
    "tier": "string";
    "label": "string";
    "status": "string";
    "slotLimit": "number";
    "attachedCount": "number";
    "availableSlots": "number";
    "paidThroughDate": "string";
});

output_schema!(HostingSubscriptionAttachment {
    "subscriptionId": "string";
    "hostingProduct": "string";
    "attachState": "string";
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
});

output_schema!(HostingAppOperation {
    "operationId": "string";
    "status": "string";
});

output_schema!(HostingApplicationStatus {
    "status": "string";
    "variants": "array";
});

output_schema!(HostingSourceImport {
    "id": "string";
    "importType": "string";
    "status": "string";
    "createdAt": "string";
});

output_schema!(HostingDomain {
    "id": "string";
    "hostname": "string";
    "role": "string";
    "verificationStatus": "string";
});

output_schema!(HostingGitHubProfile {
    "connected": "boolean";
    "appInstalled": "boolean";
    "appInstallUrl": "string";
});

output_schema!(HostingRepository {
    "fullName": "string";
});

output_schema!(HostingBranch {
    "name": "string";
});

output_schema!(HostingRuntime {
    "runtime": "string";
    "version": "string";
});

pub fn client_err(e: ClientError) -> CliCoreError {
    crate::error::GddyError::from(e).into_cli_error()
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

pub fn parse_app_type(s: &str) -> Result<String, String> {
    match s.to_uppercase().as_str() {
        "NODEJS" | "MHWP" => Ok(s.to_uppercase()),
        _ => Err(format!("unknown app type {s:?} — expected NODEJS")),
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
    use serde_json::json;

    #[test]
    fn parse_app_type_accepts_known_types_case_insensitive() {
        assert_eq!(parse_app_type("NODEJS").expect("NODEJS"), "NODEJS");
        assert_eq!(parse_app_type("nodejs").expect("nodejs"), "NODEJS");
        assert_eq!(parse_app_type("MHWP").expect("MHWP"), "MHWP");
    }

    #[test]
    fn parse_app_type_rejects_unknown() {
        assert!(parse_app_type("UNKNOWN").is_err());
        assert!(parse_app_type("").is_err());
    }

    #[test]
    fn next_page_token_extracts_from_links() {
        let response = json!({
            "items": [],
            "links": [
                { "rel": "self", "href": "https://api.godaddy.com/v1/hosting/apps?appType=NODEJS" },
                { "rel": "next", "href": "https://api.godaddy.com/v1/hosting/apps?appType=NODEJS&pageToken=tok-2&limit=10" }
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
}
