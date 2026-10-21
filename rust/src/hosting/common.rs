use clap::builder::{PossibleValuesParser, TypedValueParser};
use cli_engine::{
    CliCoreError, CommandContext, CommandSpec, CursorContinuation, FlagPolicy, Stage,
};
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
    if matches!(e, ClientError::Http { status: 501, .. }) {
        return client_err_with_fix(
            e,
            "This operation is not supported for this app type or hosting product.",
        );
    }
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

/// Returns the `details[].issue` codes of an API error.
pub fn error_issues(e: &ClientError) -> Vec<String> {
    let ClientError::Http { body, .. } = e else {
        return Vec::new();
    };
    serde_json::from_str::<ApiErrorBody>(body)
        .map(|b| b.details.into_iter().filter_map(|d| d.issue).collect())
        .unwrap_or_default()
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
    crate::http::ensure_generated_client_transport_observer_registered();
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
    #[value(name = "MHWP")]
    Mhwp,
}

impl HostingAppType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Nodejs => "NODEJS",
            Self::Mhwp => "MHWP",
        }
    }
}

/// Feature flag for the MHWP app type. Separate from `hosting` so MHWP
/// can stay hidden until its backend is live.
const MHWP_FLAG: &str = "hosting-mhwp";

pub fn mhwp_enabled(policy: &FlagPolicy) -> bool {
    policy.visible(Some(MHWP_FLAG), Stage::Experimental)
}

/// App types to list in help text.
pub const fn supported_app_types(mhwp: bool) -> &'static str {
    if mhwp { "NODEJS, MHWP" } else { "NODEJS" }
}

/// Like `CommandSpec::from_args`, but `--app-type` accepts MHWP only
/// while its feature flag is on.
pub fn app_type_command<T: clap::Args>(name: &str, short: &str, mhwp: bool) -> CommandSpec {
    let mut spec = CommandSpec::from_args::<T>(name, short);
    if !mhwp {
        for arg in spec.args.iter_mut().filter(|a| a.get_id() == "app_type") {
            *arg = arg.clone().value_parser(
                PossibleValuesParser::new(["NODEJS"]).map(|_| HostingAppType::Nodejs),
            );
        }
    }
    spec
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum HostingProduct {
    #[value(name = "WEB_HOSTING")]
    WebHosting,
    #[value(name = "MANAGED_WORDPRESS")]
    ManagedWordpress,
}

impl HostingProduct {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WebHosting => "WEB_HOSTING",
            Self::ManagedWordpress => "MANAGED_WORDPRESS",
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

/// The `href` of a paged response's `links[rel=next]`, if it has one.
pub fn next_link_href(response: &Value) -> Option<String> {
    response
        .get("links")?
        .as_array()?
        .iter()
        .find(|link| link.get("rel").and_then(Value::as_str) == Some("next"))?
        .get("href")?
        .as_str()
        .map(str::to_owned)
}

/// The warning to show when a listing that this CLI can only read one page of
/// reports another page via `links[rel=next]` — for the endpoints whose spec
/// types the response as paged but declares no paging request parameters
/// (`hosting domain list`, `hosting subscription list`). `None` when the
/// service reports nothing further.
pub fn more_pages_warning(response: &Value, what: &str) -> Option<String> {
    next_link_href(response).map(|_| {
        format!(
            "The API reports more {what} than can be displayed."
        )
    })
}

/// Writes `message` as a one-line warning to `out`.
fn write_warning(out: &mut impl std::io::Write, message: &str) {
    let _ = writeln!(out, "warning: {message}");
}

/// Shows a non-fatal warning to the user on stderr, keeping stdout's JSON/TOON
/// payload clean.
///
/// Interim: the engine has no result-level warning channel yet. Replace calls
/// with `CommandResult::with_warning` once cli-engine#131 is released and
/// adopted.
pub fn warn_user(message: &str) {
    write_warning(&mut std::io::stderr(), message);
}

/// The `items` array of a paged hosting response (empty when absent).
pub fn page_items(response: &Value) -> Vec<Value> {
    response
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// Maps a paged hosting response's `links[rel=next]` onto the engine's cursor
/// continuation, using the server's own `pageToken` as the `--continue` token.
pub fn page_continuation(response: &Value) -> CursorContinuation {
    match next_page_token(response) {
        Some(token) => CursorContinuation::more(token),
        None => CursorContinuation::done(),
    }
}

/// The engine's resolved `--limit` as the `pageSize` the hosting client takes.
pub fn page_limit(cursor_limit: i64) -> Option<u32> {
    u32::try_from(cursor_limit).ok().filter(|n| *n > 0)
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
    fn hosting_app_type_parses_mhwp_case_insensitive() {
        assert_eq!(
            HostingAppType::from_str("mhwp", true).expect("mhwp"),
            HostingAppType::Mhwp
        );
        assert_eq!(HostingAppType::Mhwp.as_str(), "MHWP");
    }

    #[test]
    fn mhwp_is_off_until_experimental_or_overridden() {
        assert!(!mhwp_enabled(&FlagPolicy::new()));
        assert!(!mhwp_enabled(
            &FlagPolicy::new().with_min_stage(Stage::Beta)
        ));
        assert!(mhwp_enabled(
            &FlagPolicy::new().with_min_stage(Stage::Experimental)
        ));
        assert!(mhwp_enabled(
            &FlagPolicy::new().with_override(MHWP_FLAG, Stage::Ga)
        ));
    }

    #[test]
    fn supported_app_types_lists_mhwp_only_when_enabled() {
        assert_eq!(supported_app_types(false), "NODEJS");
        assert_eq!(supported_app_types(true), "NODEJS, MHWP");
    }

    #[test]
    fn hosting_app_type_rejects_unknown() {
        assert!(HostingAppType::from_str("UNKNOWN", true).is_err());
        assert!(HostingAppType::from_str("", true).is_err());
    }

    #[test]
    fn hosting_product_parses_known_values() {
        assert_eq!(
            HostingProduct::from_str("WEB_HOSTING", true).expect("WEB_HOSTING"),
            HostingProduct::WebHosting
        );
        assert_eq!(
            HostingProduct::from_str("managed_wordpress", true).expect("managed_wordpress"),
            HostingProduct::ManagedWordpress
        );
        assert_eq!(HostingProduct::WebHosting.as_str(), "WEB_HOSTING");
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
    fn more_pages_warning_appears_only_when_the_api_reports_a_next_page() {
        let more = json!({
            "items": [],
            "links": [{ "rel": "next", "href": "/v1/hosting/apps/a/domains?pageToken=t2" }]
        });
        let warning = more_pages_warning(&more, "domains").expect("next link warns");
        assert!(warning.contains("more domains"), "{warning}");
        assert!(warning.contains("incomplete"), "{warning}");

        let complete = json!({
            "items": [],
            "links": [{ "rel": "self", "href": "/v1/hosting/apps/a/domains" }]
        });
        assert_eq!(more_pages_warning(&complete, "domains"), None);
        assert_eq!(more_pages_warning(&json!({ "items": [] }), "domains"), None);
    }

    #[test]
    fn write_warning_emits_one_prefixed_line() {
        let mut out = Vec::new();
        write_warning(&mut out, "list may be incomplete");
        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            "warning: list may be incomplete\n"
        );
    }

    #[test]
    fn page_items_returns_the_items_array_or_empty() {
        assert_eq!(
            page_items(&json!({ "items": [{ "id": "a" }, { "id": "b" }] })).len(),
            2
        );
        assert!(page_items(&json!({})).is_empty());
    }

    #[test]
    fn page_continuation_carries_the_servers_page_token_and_is_done_without_one() {
        let more = page_continuation(&json!({
            "links": [{ "rel": "next", "href": "https://api.godaddy.com/v1/hosting/apps?pageToken=tok-2" }]
        }));
        assert_eq!(more.continue_from.as_deref(), Some("tok-2"));
        assert_eq!(
            page_continuation(&json!({ "items": [] })).continue_from,
            None
        );
    }

    #[test]
    fn next_link_href_picks_the_next_relation_only() {
        let response = json!({
            "links": [
                { "rel": "self", "href": "/v1/hosting/apps/a/domains" },
                { "rel": "next", "href": "/v1/hosting/apps/a/domains?pageToken=tok-2" }
            ]
        });
        assert_eq!(
            next_link_href(&response).as_deref(),
            Some("/v1/hosting/apps/a/domains?pageToken=tok-2")
        );
        assert_eq!(
            next_link_href(&json!({ "links": [{ "rel": "self", "href": "/x" }] })),
            None
        );
        assert_eq!(next_link_href(&json!({ "items": [] })), None);
    }

    #[test]
    fn page_limit_rejects_non_positive_and_oversized_values() {
        assert_eq!(page_limit(50), Some(50));
        assert_eq!(page_limit(0), None);
        assert_eq!(page_limit(-1), None);
        assert_eq!(page_limit(i64::MAX), None);
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
            "Run: gddy hosting subscription list --hosting-product=WEB_HOSTING",
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
            Some("Run: gddy hosting subscription list --hosting-product=WEB_HOSTING")
        );
    }

    #[test]
    fn error_issues_returns_every_issue_code() {
        let err = ClientError::Http {
            status: 422,
            body: r#"{"message":"m","details":[{"issue":"VALIDATION_FAILED"},{"description":"d"},{"issue":"APP_LIMIT_EXCEEDED"}]}"#
                .to_owned(),
        };
        assert_eq!(
            error_issues(&err),
            ["VALIDATION_FAILED", "APP_LIMIT_EXCEEDED"]
        );
        let plain = ClientError::Http {
            status: 500,
            body: "not json".to_owned(),
        };
        assert!(error_issues(&plain).is_empty());
        assert!(error_issues(&ClientError::Network("down".to_owned())).is_empty());
    }

    #[test]
    fn client_err_explains_501_not_applicable() {
        let err = client_err(ClientError::Http {
            status: 501,
            body: r#"{"message":"Not supported for MHWP apps.","details":[{"issue":"NOT_APPLICABLE"}]}"#
                .to_owned(),
        });
        let envelope = cli_engine::build_error_envelope(&err, "hosting");
        assert_eq!(
            envelope.error.as_ref().map(|e| e.message.as_str()),
            Some("HTTP error 501: Not supported for MHWP apps. (NOT_APPLICABLE)")
        );
        assert_eq!(
            envelope.fix.as_deref(),
            Some("This operation is not supported for this app type or hosting product.")
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
