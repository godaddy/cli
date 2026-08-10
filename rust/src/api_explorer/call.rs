//! `api call` — makes an authenticated HTTP request against any endpoint in
//! the embedded catalog (or an unmatched path, falling back to the generic
//! gateway host).

use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use serde_json::{Map, Value, json};

use super::catalog::{catalog, find_endpoint};
use super::http::{graphql_errors, is_mutating_method, merge_required_scopes, split_header};

#[derive(Debug, Clone, clap::Args)]
struct CallArgs {
    /// Relative API path (e.g. /v1/commerce/stores/{storeId}/orders).
    #[arg(value_name = "ENDPOINT")]
    endpoint: String,

    /// HTTP method.
    #[arg(long, short = 'X', value_name = "METHOD", default_value = "GET")]
    method: String,

    /// Request body as raw JSON string.
    #[arg(long, short = 'd', value_name = "JSON")]
    body: Option<String>,

    /// Add a field to the request body (key=value, repeatable).
    #[arg(long, short = 'f', value_name = "KEY=VALUE")]
    field: Vec<String>,

    /// Read request body from a JSON file.
    #[arg(long, short = 'F', value_name = "PATH")]
    file: Option<String>,

    /// Extra request headers.
    #[arg(long, short = 'H', value_name = "KEY:VALUE")]
    header: Vec<String>,

    /// Include response headers in output.
    #[arg(long, short = 'i')]
    include: bool,

    /// Additional required OAuth scope(s), merged with the endpoint's.
    // One value per occurrence, repeatable (`--scope a --scope b`) — a `Vec`
    // field defaults to append-style, not `num_args(1..)`, so it can't
    // greedily consume the ENDPOINT positional either.
    #[arg(long, short = 's', value_name = "SCOPE")]
    scope: Vec<String>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<CallArgs, _, _, _>(
        CommandSpec::from_args::<CallArgs>("call", "Make an authenticated API request")
            .with_long(
                "Executes an authenticated HTTP request against any GoDaddy API endpoint. \
                 Supply the request body as raw JSON (`--body '{...}'`), as individual \
                 fields (`--field key=value`, repeatable), or from a JSON file \
                 (`--file body.json`); `--file` takes precedence over `--body`, \
                 and `--field` values are merged on top of either. Use the global \
                 `--expr`/`--filter` flags (JMESPath) to extract or filter \
                 response data, and `--include` to see response headers \
                 alongside the body. Use `api operation get <operationId>` \
                 to inspect required parameters and scopes.",
            )
            .with_system("api")
            .with_tier(Tier::Mutate)
            .handles_dry_run(true)
            // The dry-run-for-mutating-methods branch never needs a token, so
            // resolution is deferred to the handler (which only calls
            // `credential_with_scopes` on the path that actually sends a
            // request) instead of the engine resolving eagerly beforehand.
            .auth_optional(),
        |ctx, args: CallArgs| async move {
            let endpoint = args.endpoint.as_str();
            let method = args.method.as_str();
            // Validate the method unconditionally, including under
            // `--dry-run` — otherwise a garbage/malformed method (e.g.
            // "POST " with trailing whitespace) would return a successful
            // dry-run preview even though a real run would reject it here.
            let parsed_method: reqwest::Method = method.parse().map_err(|_| {
                crate::error::GddyError::validation(format!("invalid HTTP method: {method}"))
                    .into_cli_error()
            })?;

            // Validate unconditionally, including under `--dry-run` (same
            // rationale as the method check above). `find_endpoint` below can
            // match a raw operationId, but the request URL is always built
            // from this literal `endpoint` string, not the matched catalog
            // path — accepting a bare operationId here would silently build
            // an invalid URL (e.g. `https://.../listFulfillments`).
            if !endpoint.starts_with('/') {
                return Err(crate::error::GddyError::validation(format!(
                    "endpoint must be a URL path starting with '/', not {endpoint:?} — \
                     use `api operation get {endpoint}` to find the concrete path"
                ))
                .into_cli_error());
            }

            // `--dry-run` is statically tagged `Tier::Mutate` since the method
            // is only known at runtime, but a GET/HEAD is safe to actually run
            // (and more useful previewed as real data than as a generic
            // string) — only short-circuit for methods that would mutate.
            if ctx.dry_run() && is_mutating_method(method) {
                return Ok(CommandResult::new(json!({
                    "command": "api:call",
                    "action": "dry-run: would execute",
                    "method": method,
                    "endpoint": endpoint,
                }))
                .with_dry_run());
            }

            // Best-effort match against the embedded catalog: a concrete request
            // path may not match a templated catalog path, in which case scopes
            // fall back to just --scope and the base URL falls back to the
            // generic gateway host.
            let matched = find_endpoint(catalog(), endpoint);

            // Required scopes = explicit --scope flags, plus the matched
            // endpoint's declared scopes. These drive OAuth scope step-up at
            // credential time.
            let flag_scopes = args.scope.clone();
            let endpoint_scopes = matched.map(|(_, ep)| ep.scopes.as_slice()).unwrap_or(&[]);
            let required = merge_required_scopes(flag_scopes, endpoint_scopes);

            let token = ctx.credential_with_scopes(&required).await?.token;
            let base_url = match matched {
                Some((domain, _)) => crate::environments::resolve_catalog_base_url(
                    &domain.name,
                    &domain.base_url,
                    &ctx.middleware.env,
                ),
                None => crate::application::client::api_url_for_env(&ctx.middleware.env)?,
            };
            let url = format!("{base_url}{endpoint}");

            // Build request body: -F file > -d body, then merge -f fields on top
            let mut request_body: Option<Value> = None;

            if let Some(file_path) = args.file.as_deref() {
                let content = std::fs::read_to_string(file_path).map_err(|e| {
                    crate::error::GddyError::validation(format!(
                        "failed to read file '{file_path}': {e}"
                    ))
                    .into_cli_error()
                })?;
                request_body = Some(serde_json::from_str(&content).map_err(|e| {
                    crate::error::GddyError::validation(format!(
                        "invalid JSON in '{file_path}': {e}"
                    ))
                    .into_cli_error()
                })?);
            } else if let Some(body_str) = args.body.as_deref() {
                request_body = Some(serde_json::from_str(body_str).map_err(|e| {
                    crate::error::GddyError::validation(format!("invalid JSON body: {e}"))
                        .into_cli_error()
                })?);
            }

            let fields = &args.field;
            if !fields.is_empty() {
                let body = request_body.get_or_insert_with(|| json!({}));
                for s in fields {
                    let eq = s.find('=').ok_or_else(|| {
                        crate::error::GddyError::validation(format!(
                            "invalid field format '{s}': expected key=value"
                        ))
                        .into_cli_error()
                    })?;
                    let key = s[..eq].to_owned();
                    let val = s[eq + 1..].to_owned();
                    if let Some(obj) = body.as_object_mut() {
                        obj.insert(key, json!(val));
                    }
                }
            }

            let client = crate::application::client::make_http_client();
            let mut req = client
                .request(parsed_method, &url)
                .bearer_auth(&token)
                .header("x-request-id", uuid::Uuid::new_v4().to_string());

            // Apply user-supplied `--header KEY:VALUE` values (repeatable).
            for h in &args.header {
                let (key, val) = split_header(h).ok_or_else(|| {
                    crate::error::GddyError::validation(format!(
                        "invalid header '{h}': expected KEY:VALUE"
                    ))
                    .into_cli_error()
                })?;
                req = req.header(key, val);
            }

            if let Some(body) = request_body {
                req = req.json(&body);
            }

            let request = req
                .build()
                .map_err(|e| crate::error::GddyError::validation(e.to_string()))?;
            cli_engine::transport::debug_log_reqwest_request(&request);
            let resp = client
                .execute(request)
                .await
                .map_err(|e| crate::error::GddyError::network(e.to_string()))?;

            let status_code = resp.status();
            let status_text = status_code.canonical_reason().unwrap_or("").to_owned();
            let response_headers_raw = resp.headers().clone();
            let include_headers = args.include;
            let body_bytes = resp
                .bytes()
                .await
                .map_err(|e| crate::error::GddyError::network(e.to_string()))?;
            cli_engine::transport::debug_log_reqwest_response(
                status_code,
                &response_headers_raw,
                &body_bytes,
            );

            let status = status_code.as_u16();
            let response_headers: Option<Map<String, Value>> = if include_headers {
                Some(
                    response_headers_raw
                        .iter()
                        .filter_map(|(k, v)| v.to_str().ok().map(|s| (k.to_string(), json!(s))))
                        .collect(),
                )
            } else {
                None
            };

            let body: Value = super::http::parse_response_body(&body_bytes);

            // GraphQL endpoints return HTTP 200 even on failure, carrying the error
            // in a top-level `errors` array. Detect the GraphQL commerce surfaces by
            // path and surface those errors instead of reporting a false success.
            let is_graphql = endpoint.contains("graphql") || endpoint.contains("subgraph");
            if is_graphql && let Some(errors) = graphql_errors(&body) {
                return Err(crate::error::GddyError::from_graphql(
                    format!(
                        "GraphQL request returned {} error(s):\n{}",
                        errors.len(),
                        serde_json::to_string_pretty(&json!(errors)).unwrap_or_default(),
                    ),
                    "api",
                )
                .into());
            }

            // Scope step-up already ran up front (the token was requested with
            // `required`). A 403 here means the granted token still lacks a
            // required scope — surface it with a re-login hint.
            if status == 403 && !required.is_empty() {
                // `auth login --scope` is append-style (one value per flag), so
                // repeat the flag rather than space-joining.
                let login_hint = required
                    .iter()
                    .map(|s| format!("--scope {s}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                return Err(crate::error::GddyError::auth(format!(
                    "403 Forbidden — the authorized token is missing required scope(s): {}. \
                     Re-run `gddy auth login {login_hint}` and try again.",
                    required.join(", "),
                ))
                .with_fix(format!("Run: gddy auth login {login_hint}"))
                .into());
            }

            // Any other non-2xx is a failure, not a success envelope. Include the
            // status line and (truncated) response body so the caller sees the detail
            // instead of a success result that happens to carry an error payload.
            if !(200..300).contains(&status) {
                let detail: String = serde_json::to_string_pretty(&body)
                    .unwrap_or_else(|_| body.to_string())
                    .chars()
                    .take(4000)
                    .collect();
                return Err(crate::error::GddyError::from_http(
                    status,
                    format!("{status_text}\n{detail}"),
                    "api",
                )
                .into_cli_error());
            }

            // Identify the call and its outcome in the result envelope.
            let mut result = json!({
                "endpoint": endpoint,
                "method": method,
                "status": status,
                "status_text": status_text,
                "data": body,
            });
            if let Some(headers) = response_headers {
                result["headers"] = Value::Object(headers);
            }

            Ok(CommandResult::new(result))
        },
    )
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig};

    /// `call` opted into handler-driven `--dry-run` so a GET/HEAD can execute
    /// for real under `--dry-run` — but it still requires `Required` auth
    /// (no `.no_auth()`), so it must stay fail-closed regardless of method.
    #[tokio::test]
    async fn call_dry_run_get_still_requires_auth() {
        const AUTH_FAILURE_EXIT: i32 = 2;
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_default_auth_provider("godaddy")
                .with_module(super::super::module()),
        );
        let output = cli
            .run([
                "gddy",
                "api",
                "call",
                "/v1/example",
                "--method",
                "GET",
                "--dry-run",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(
            output.exit_code, AUTH_FAILURE_EXIT,
            "a GET falls through to the real request even under --dry-run, so it still needs \
             auth, got: {}",
            output.rendered
        );
    }

    /// `call` is `auth_optional()` specifically so the dry-run-for-mutating-
    /// methods branch never triggers credential resolution (previously it did,
    /// via the engine's `Required`-auth eager resolution before the handler
    /// even ran) — a POST preview must succeed with no auth provider at all.
    #[tokio::test]
    async fn call_dry_run_mutating_method_needs_no_auth() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_module(super::super::module()),
        );
        let output = cli
            .run([
                "gddy",
                "api",
                "call",
                "/v1/example",
                "--method",
                "POST",
                "--dry-run",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"]["action"], "dry-run: would execute");
    }

    /// A malformed method must still be rejected under `--dry-run`, matching
    /// what the real path's `method.parse()` would do — a garbage method
    /// must not previously have returned a fake "would execute" success.
    #[tokio::test]
    async fn call_dry_run_rejects_a_malformed_method() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_module(super::super::module()),
        );
        let output = cli
            .run([
                "gddy",
                "api",
                "call",
                "/v1/example",
                "--method",
                "POST ",
                "--dry-run",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            output.rendered.contains("invalid HTTP method"),
            "{}",
            output.rendered
        );
    }

    /// A bare operationId (no leading '/') must be rejected rather than
    /// silently built into an invalid request URL — `find_endpoint` can match
    /// it for scope lookup, but the URL is always built from the literal
    /// `endpoint` string, not the matched catalog path.
    #[tokio::test]
    async fn call_rejects_an_endpoint_without_a_leading_slash() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_module(super::super::module()),
        );
        let output = cli
            .run([
                "gddy",
                "api",
                "call",
                "listFulfillments",
                "--dry-run",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            output.rendered.contains("must be a URL path starting with"),
            "{}",
            output.rendered
        );
    }
}
