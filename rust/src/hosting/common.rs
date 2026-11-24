use clap::ValueEnum;
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
    // Why a FAILED deployment did not complete: `message`, `details[].issue`,
    // and `links` (rel `upgrade` when the plan has to be upgraded).
    "error": "object", optional;
    // Build and publish log lines captured so far, oldest first.
    "logs": "array", optional;
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
    // What hosts a Node.js app: WEB_HOSTING or AIRO. Left out for WordPress.
    "hostingProduct": "string", optional;
    "source": "string", optional;
    "sourceDetails": "object", optional;
});

output_schema!(HostingAppOperation {
    "operationId": "string";
    "status": "string";
    // The kind of operation, such as CREATE_APP or SET_PUBLISH_STATE;
    // `with_operation_type` fills it in where the service leaves it out.
    "type": "string";
    "app": "object", optional;
    "error": "object", optional;
    "createdAt": "string", optional;
    "links": "array";
});

// `hosting app publish` answers with what the product's own go-live call
// returns: a deployment for a Node.js app, an operation for a WordPress site.
output_schema!(HostingAppPublish {
    "deploymentId": "string", optional;
    "operationId": "string", optional;
    "status": "string";
    "type": "string", optional;
    "gitHash": "string", optional;
    "error": "object", optional;
    "createdAt": "string", optional;
    "updatedAt": "string", optional;
    "links": "array", optional;
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
        ClientError::Http { status, body, .. } => {
            GddyError::from_http(status, format_api_error_body(&body), "hosting").into_cli_error()
        }
        other => GddyError::from(other).into_cli_error(),
    }
}

/// Like [`client_err`], but overrides the fix hint.
pub fn client_err_with_fix(e: ClientError, fix: impl Into<String>) -> CliCoreError {
    match e {
        ClientError::Http { status, body, .. } => {
            GddyError::from_http(status, format_api_error_body(&body), "hosting")
                .with_fix(fix)
                .into_cli_error()
        }
        other => GddyError::from(other).into_cli_error(),
    }
}

/// [`client_err`], except that a `503` on a WordPress write says to retry.
/// The service answers `503` both when another change holds the site and when
/// a dependency is down, and only the first sets `Retry-After`, which is not
/// used here, so the text has to be true for either.
pub fn client_err_retry_on_503(e: ClientError, mhwp: bool) -> CliCoreError {
    if mhwp && matches!(e, ClientError::Http { status: 503, .. }) {
        return client_err_with_fix(
            e,
            "The service is temporarily unavailable. Wait a few seconds and run the command again.",
        );
    }
    client_err(e)
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

/// The `href` of an API error's `links[rel]` entry: where the service sends
/// the customer to resolve the error. See [`printable_link`].
pub fn error_link(e: &ClientError, rel: &str) -> Option<String> {
    let ClientError::Http { body, .. } = e else {
        return None;
    };
    // Read apart from `ApiErrorBody`, so that a `links` value of another shape
    // cannot hide the error's message and issue codes.
    printable_link(&serde_json::from_str(body).ok()?, rel)
}

/// The `href` of the `links[rel]` entry of `error`, an API error object such
/// as an error response or the `error` of a failed deployment. The link is
/// printed as it is, so only an absolute `https` URL of visible ASCII
/// characters with a host and no user name or password (no `@` before the
/// path) is returned.
pub fn printable_link(error: &Value, rel: &str) -> Option<String> {
    error
        .get("links")?
        .as_array()?
        .iter()
        .filter(|link| link.get("rel").and_then(Value::as_str) == Some(rel))
        .filter_map(|link| link.get("href").and_then(Value::as_str))
        .find(|href| is_safe_to_print_https_url(href))
        .map(str::to_owned)
}

fn is_safe_to_print_https_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    if !rest.bytes().all(|b| b.is_ascii_graphic()) {
        return false;
    }
    // Raw check too: the parser turns `https:///x` into host `x` and drops an
    // empty `@` user name.
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    !authority.is_empty()
        && !authority.contains('@')
        && url::Url::parse(url).is_ok_and(|parsed| parsed.host_str().is_some())
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

    /// The prefix that starts every app and operation id of this type.
    pub const fn id_prefix(self) -> &'static str {
        match self {
            Self::Nodejs => "NODEJS-",
            Self::Mhwp => "MHWP-",
        }
    }

    /// The type an app or operation id names by its prefix, if any.
    pub fn of_id(id: &str) -> Option<Self> {
        Self::value_variants()
            .iter()
            .copied()
            .find(|app_type| strip_id_prefix(id, app_type.id_prefix()).is_some())
    }
}

/// The `type` of an app operation that creates an app.
pub const CREATE_APP: &str = "CREATE_APP";
/// The `type` of an app operation that takes a site live or offline.
pub const SET_PUBLISH_STATE: &str = "SET_PUBLISH_STATE";

/// `operation` with its `type` set to `kind` where the service left it out,
/// so the output always says which kind of operation it is. Anything that is
/// not an object, such as an empty `202`, comes back unchanged.
pub fn with_operation_type(mut operation: Value, kind: &str) -> Value {
    if let Some(fields) = operation.as_object_mut()
        && fields.get("type").is_none_or(Value::is_null)
    {
        fields.insert("type".to_owned(), Value::from(kind));
    }
    operation
}

/// `app_id` with `prefix` removed, or `None` if it does not start with it.
/// The prefix matches ignoring ASCII case, as the edge does.
pub fn strip_id_prefix<'a>(app_id: &'a str, prefix: &str) -> Option<&'a str> {
    let (head, rest) = app_id.split_at_checked(prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then_some(rest)
}

/// Whether `app_id` names a WordPress (MHWP) site.
pub fn is_mhwp_app_id(app_id: &str) -> bool {
    HostingAppType::of_id(app_id) == Some(HostingAppType::Mhwp)
}

/// A Node.js id ends in a nanoid. Only its alphabet is checked, which keeps the
/// id URL-safe; the contract's examples disagree on the length, so the API
/// judges the rest.
pub fn is_nodejs_app_id(value: &str) -> bool {
    strip_id_prefix(value, HostingAppType::Nodejs.id_prefix()).is_some_and(|body| {
        !body.is_empty()
            && body
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    })
}

/// The edge route's id shape, `(?i)^mhwp-[a-z0-9]{10}$`: the prefix matches
/// case-insensitively, the ten characters after it do not. Anything else never
/// reaches the service, so it is refused here rather than sent out to collect
/// a 404.
pub fn is_wordpress_site_id(value: &str) -> bool {
    strip_id_prefix(value, HostingAppType::Mhwp.id_prefix()).is_some_and(|body| {
        body.len() == 10 && body.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9'))
    })
}

/// `--app-id <app_id>`, for a command written out in an error's fix: the id
/// itself when it has the shape of a Node.js app or WordPress site id, whose
/// letters, digits, `_` and `-` a shell takes as they are, and the
/// `<app-id>` placeholder for anything else.
pub fn app_id_arg(app_id: &str) -> String {
    let id = if is_nodejs_app_id(app_id) || is_wordpress_site_id(app_id) {
        app_id
    } else {
        "<app-id>"
    };
    format!("--app-id {id}")
}

/// Markdown for `gddy guide hosting`, the index of the per-product guides.
pub const HOSTING_GUIDE: &str = include_str!("guides/hosting.md");
/// Markdown for `gddy guide hosting-nodejs`.
pub const NODEJS_GUIDE: &str = include_str!("guides/hosting-nodejs.md");
/// Markdown for `gddy guide hosting-wordpress`.
pub const WORDPRESS_GUIDE: &str = include_str!("guides/hosting-wordpress.md");

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

/// The product that hosts a new Node.js app, for `hosting app create`. Not the
/// same set as [`HostingProduct`], which filters `hosting subscription list`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum AppHostingProduct {
    #[value(name = "WEB_HOSTING")]
    WebHosting,
    #[value(name = "AIRO")]
    Airo,
}

impl AppHostingProduct {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WebHosting => "WEB_HOSTING",
            Self::Airo => "AIRO",
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
    fn hosting_app_type_parses_mhwp_case_insensitive() {
        assert_eq!(
            HostingAppType::from_str("mhwp", true).expect("mhwp"),
            HostingAppType::Mhwp
        );
        assert_eq!(HostingAppType::Mhwp.as_str(), "MHWP");
    }

    #[test]
    fn is_mhwp_app_id_matches_the_prefix_case_insensitively() {
        for id in ["MHWP-a1b2c3d4e5", "mhwp-a1b2c3d4e5", "Mhwp-x"] {
            assert!(is_mhwp_app_id(id), "{id}");
        }
        for id in [
            "NODEJS-a1b2c3d4e5",
            "MHWP",
            "x-MHWP-abc",
            "",
            "MHWPa1b2c3d4e5",
            "MHWP\u{e9}",
        ] {
            assert!(!is_mhwp_app_id(id), "{id}");
        }
    }

    #[test]
    fn with_operation_type_fills_in_only_a_missing_type() {
        let filled = |operation: Value| with_operation_type(operation, CREATE_APP);
        assert_eq!(
            filled(json!({ "operationId": "op-1" })),
            json!({ "operationId": "op-1", "type": "CREATE_APP" })
        );
        assert_eq!(
            filled(json!({ "operationId": "op-1", "type": null })),
            json!({ "operationId": "op-1", "type": "CREATE_APP" })
        );
        // A kind the service sent, known or not, is kept as it is.
        for kind in ["SET_PUBLISH_STATE", "A_FUTURE_KIND"] {
            let operation = json!({ "operationId": "op-1", "type": kind });
            assert_eq!(filled(operation.clone()), operation);
        }
        // An empty response stays empty rather than turning into an operation.
        assert_eq!(filled(Value::Null), Value::Null);
    }

    #[test]
    fn of_id_reads_the_type_off_the_prefix() {
        for (id, expected) in [
            ("NODEJS-aBcDeFgHiJ", Some(HostingAppType::Nodejs)),
            ("nodejs-x", Some(HostingAppType::Nodejs)),
            ("MHWP-a1b2c3d4e5", Some(HostingAppType::Mhwp)),
            ("Mhwp-ab12cd34", Some(HostingAppType::Mhwp)),
            ("MHWP", None),
            ("NODEJS", None),
            ("x-MHWP-a1b2c3d4e5", None),
            ("MHWP\u{e9}", None),
            ("3f2a9c1e-77d0-4b1a-9c55-0e6f1a2b3c4d", None),
            ("", None),
        ] {
            assert_eq!(HostingAppType::of_id(id), expected, "{id:?}");
        }
    }

    #[test]
    fn strip_id_prefix_returns_the_rest_without_slicing_mid_character() {
        assert_eq!(strip_id_prefix("mhwp-abc", "MHWP-"), Some("abc"));
        assert_eq!(strip_id_prefix("MHWP-", "MHWP-"), Some(""));
        assert_eq!(strip_id_prefix("NODEJS-abc", "MHWP-"), None);
        assert_eq!(strip_id_prefix("MHW", "MHWP-"), None);
        // The fifth byte falls inside `é`, so there is no prefix to compare.
        assert_eq!(strip_id_prefix("MHWP\u{e9}", "MHWP-"), None);
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
    fn app_hosting_product_parses_known_values_only() {
        for (value, expected) in [
            ("WEB_HOSTING", AppHostingProduct::WebHosting),
            ("airo", AppHostingProduct::Airo),
        ] {
            let parsed = AppHostingProduct::from_str(value, true).expect(value);
            assert_eq!(parsed, expected);
            assert!(parsed.as_str().eq_ignore_ascii_case(value));
        }
        // The subscription filter's value is not a product an app is created on.
        assert!(AppHostingProduct::from_str("MANAGED_WORDPRESS", true).is_err());
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
                retry_after: None,
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
    fn a_503_on_a_wordpress_write_says_to_retry_whatever_the_cause() {
        let unavailable = || ClientError::Http {
            status: 503,
            body: r#"{"message":"m","details":[{"issue":"UPSTREAM_UNAVAILABLE"}]}"#.to_owned(),
            retry_after: None,
        };
        let fix = |e, mhwp| {
            cli_engine::build_error_envelope(&client_err_retry_on_503(e, mhwp), "hosting").fix
        };
        let wordpress = fix(unavailable(), true);
        assert!(
            wordpress
                .as_deref()
                .is_some_and(|f| f.contains("temporarily unavailable") && f.contains("few seconds")),
            "{wordpress:?}"
        );
        assert!(
            !wordpress
                .as_deref()
                .unwrap_or("")
                .contains("another change"),
            "{wordpress:?}"
        );
        let nodejs = fix(unavailable(), false);
        assert!(
            nodejs
                .as_deref()
                .is_some_and(|f| !f.contains("temporarily unavailable")),
            "{nodejs:?}"
        );
        let other = fix(
            ClientError::Http {
                status: 404,
                body: r#"{"message":"m"}"#.to_owned(),
                retry_after: None,
            },
            true,
        );
        assert!(
            other
                .as_deref()
                .is_some_and(|f| !f.contains("temporarily unavailable")),
            "{other:?}"
        );
    }

    #[test]
    fn error_issues_returns_every_issue_code() {
        let err = ClientError::Http {
            status: 422,
            body: r#"{"message":"m","details":[{"issue":"VALIDATION_FAILED"},{"description":"d"},{"issue":"APP_LIMIT_EXCEEDED"}]}"#
                .to_owned(),
            retry_after: None,
        };
        assert_eq!(
            error_issues(&err),
            ["VALIDATION_FAILED", "APP_LIMIT_EXCEEDED"]
        );
        let plain = ClientError::Http {
            status: 500,
            body: "not json".to_owned(),
            retry_after: None,
        };
        assert!(error_issues(&plain).is_empty());
        assert!(error_issues(&ClientError::Network("down".to_owned())).is_empty());
    }

    #[test]
    fn error_link_returns_the_href_of_the_named_link() {
        let http = |body: &str| ClientError::Http {
            status: 422,
            body: body.to_owned(),
            retry_after: None,
        };
        let err = http(
            r#"{"message":"m","details":[{"issue":"PLAN_UPGRADE_REQUIRED"}],"links":[
                {"rel":"self","href":"https://api.godaddy.com/x"},
                {"rel":"upgrade","href":"https://www.godaddy.com/airo/plans/NODEJS-aBcDeFgHiJ"}]}"#,
        );
        assert_eq!(
            error_link(&err, "upgrade").as_deref(),
            Some("https://www.godaddy.com/airo/plans/NODEJS-aBcDeFgHiJ")
        );
        assert_eq!(error_link(&err, "next"), None);

        for href in [
            "https://www.example.com/airo/plans/NODEJS-aBcDeFgHiJ",
            "https://www.example.com:8443/airo/plans?plan=paid#upgrade",
        ] {
            let err = http(&format!(
                r#"{{"links":[{{"rel":"upgrade","href":"{href}"}}]}}"#
            ));
            assert_eq!(error_link(&err, "upgrade").as_deref(), Some(href));
        }

        for body in [
            r#"{"message":"m"}"#,
            r#"{"message":"m","links":null}"#,
            r#"{"message":"m","links":[{"rel":"upgrade"}]}"#,
            "not json",
        ] {
            assert_eq!(error_link(&http(body), "upgrade"), None, "{body}");
        }
        assert_eq!(
            error_link(&ClientError::Network("down".to_owned()), "upgrade"),
            None
        );
    }

    #[test]
    fn error_link_drops_a_link_that_is_not_safe_to_print() {
        for href in [
            "http://www.godaddy.com/airo",
            "javascript:alert(1)",
            "https://",
            "https://www.godaddy.com/a b",
            // A right-to-left override would change how the link reads.
            "https://www.godaddy.com/\u{202e}moc.live",
            // JSON escapes: an escape character and a line break.
            r"https://www.godaddy.com/\u001b[31m",
            r"https://www.godaddy.com/\n",
            "",
            // No host, or a host hidden behind a user name.
            "https://?plan=paid",
            "https://#upgrade",
            "https:///airo/plans",
            "https://www.godaddy.com@evil.example/x",
        ] {
            let err = ClientError::Http {
                status: 422,
                body: format!(r#"{{"links":[{{"rel":"upgrade","href":"{href}"}}]}}"#),
                retry_after: None,
            };
            assert_eq!(error_link(&err, "upgrade"), None, "{href:?}");
        }
    }

    /// A `links` value of an unexpected shape must not hide the issue codes.
    #[test]
    fn error_issues_survive_malformed_links() {
        let err = ClientError::Http {
            status: 422,
            body: r#"{"message":"m","details":[{"issue":"PLAN_UPGRADE_REQUIRED"}],"links":"x"}"#
                .to_owned(),
            retry_after: None,
        };
        assert_eq!(error_issues(&err), ["PLAN_UPGRADE_REQUIRED"]);
        assert_eq!(error_link(&err, "upgrade"), None);
    }

    #[test]
    fn client_err_explains_501_not_applicable() {
        let err = client_err(ClientError::Http {
            status: 501,
            body: r#"{"message":"Not supported for MHWP apps.","details":[{"issue":"NOT_APPLICABLE"}]}"#
                .to_owned(),
            retry_after: None,
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
            retry_after: None,
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
