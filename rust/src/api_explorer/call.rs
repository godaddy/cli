//! `api call` — makes an authenticated HTTP request against any endpoint in
//! the embedded catalog (or an unmatched path, falling back to the generic
//! gateway host).

use cli_engine::{CliCoreError, CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use super::catalog::{
    Endpoint, catalog, graphql_operation_redirect_error, locate_by_path, resolve_graphql_operation,
    resolve_operation,
};
use super::http::{
    encode_path_segment, is_mutating_method, merge_required_scopes, parsed_extra_headers,
    send_and_report, split_kv,
};

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

    /// Operation parameter (name=value, repeatable) — path, query, header,
    /// or body value, routed by the resolved operation's declared
    /// parameter locations (unrecognized names become body fields, like
    /// --field).
    #[arg(long, value_name = "NAME=VALUE")]
    param: Vec<String>,

    /// Extra request headers.
    #[arg(long, short = 'H', value_name = "KEY:VALUE")]
    header: Vec<String>,

    /// Include response headers in output.
    #[arg(long)]
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
                "Executes an authenticated HTTP request against any GoDaddy API \
                endpoint. ENDPOINT may be a literal path (e.g. \
                /v1/commerce/stores/{storeId}/orders) or an operation id (see \
                `api operation list --domain <domain>` or `api search`). Once \
                the endpoint is located, `--param name=value` (repeatable) \
                supplies its path, query, header, and body values, routed based \
                on declared parameters; `api operation get <operationId>` shows \
                which names are required. A value already present in a literal \
                path (a filled-in placeholder, or an existing ?name=value) \
                counts as already supplied. Supply the request body as raw JSON \
                (`--body '{...}'`), as individual fields (`--field key=value`, \
                repeatable), or from a JSON file (`--file body.json`); `--file` \
                takes precedence over `--body`, and `--field`/`--param` values are \
                merged on top of either. Use the global `--expr`/`--filter` \
                flags (JMESPath) to extract or filter response data, and \
                `--include` to see response headers alongside the body.",
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

            // Locate the (Domain, Endpoint) match, if any — a literal path
            // via the exact/template `locate_by_path`, or a bare operation id
            // via `resolve_operation`.
            let (matched, mut path) = if endpoint.starts_with('/') {
                // Catalog paths never carry a query string — strip one off
                // before matching so a literal path like `/v1/foo?limit=10`
                // still resolves, while `path` (used for the request URL and
                // for `existing_query_param_names`) keeps it.
                let path_only = endpoint.split_once('?').map_or(endpoint, |(p, _)| p);
                (
                    locate_by_path(catalog(), path_only, method)?,
                    endpoint.to_owned(),
                )
            } else {
                if resolve_graphql_operation(catalog(), endpoint).is_some() {
                    return Err(graphql_operation_redirect_error(endpoint, "graphql call"));
                }
                let (domain, ep) = resolve_operation(catalog(), endpoint, None)?;
                (Some((domain, ep)), ep.path.clone())
            };
            let ep_opt = matched.map(|(_, ep)| ep);

            // Process: partition `--param` by the matched endpoint's declared
            // parameter locations (unmatched -> body, same as --field),
            // substitute path params, and validate every declared-required
            // parameter is satisfied — by `--param`, by `--header`, or already
            // present literally in ENDPOINT. Unconditional, including under
            // `--dry-run` (same rationale as the method check above): a bad
            // `--param`/`--header` or an unresolvable/ambiguous locate must
            // fail even in a dry run, not return a fake preview.
            let parts = partition_call_params(ep_opt, &args.param)?;
            for (name, value) in &parts.path {
                path = path.replace(&format!("{{{name}}}"), &encode_path_segment(value));
            }
            let mut extra_headers = parsed_extra_headers(&args.header)?;
            extra_headers.extend(parts.header.iter().cloned());
            if let Some(ep) = ep_opt {
                validate_required_params(ep, &path, &parts, &extra_headers)?;
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
                    "path": path,
                }))
                .with_dry_run());
            }

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
            let url = format!("{base_url}{path}");
            let url = append_query(&url, &parts.query)?;

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
            if !fields.is_empty() || !parts.body.is_empty() {
                let body = request_body.get_or_insert_with(|| json!({}));
                for s in fields {
                    let (key, val) = split_kv(s).ok_or_else(|| {
                        crate::error::GddyError::validation(format!(
                            "invalid field format '{s}': expected key=value"
                        ))
                        .into_cli_error()
                    })?;
                    if let Some(obj) = body.as_object_mut() {
                        obj.insert(key.to_owned(), json!(val));
                    }
                }
                // `--param` body values are merged on top of `--field`, so a
                // repeated key resolves in `--param`'s favor.
                for (key, val) in &parts.body {
                    if let Some(obj) = body.as_object_mut() {
                        obj.insert(key.clone(), json!(val));
                    }
                }
            }

            let client = crate::application::client::make_http_client();

            // GraphQL endpoints return HTTP 200 even on failure, carrying the error
            // in a top-level `errors` array. Detect the GraphQL commerce surfaces by
            // path and surface those errors instead of reporting a false success.
            let is_graphql = path.contains("graphql") || path.contains("subgraph");

            send_and_report(
                &client,
                parsed_method,
                method,
                &url,
                &token,
                &extra_headers,
                request_body,
                args.include,
                is_graphql,
                &required,
                endpoint,
            )
            .await
        },
    )
}

/// `--param name=value` pairs partitioned by where they belong in the request,
/// per the resolved operation's declared parameters.
#[derive(Debug, Default, PartialEq)]
struct PartitionedParams {
    path: Vec<(String, String)>,
    query: Vec<(String, String)>,
    header: Vec<(String, String)>,
    body: Vec<(String, String)>,
}

/// Parses `--param NAME=VALUE` and routes each by `ep`'s declared parameter
/// locations — mode-agnostic: `ep` is `None` for an unmatched literal path,
/// in which case every value becomes a body field, same as `--field`. A
/// name matching nothing declared also becomes a body field — REST
/// `requestBody` properties aren't enumerated the way `ep.parameters` is, so
/// (like `--field`) there's no reliable way to distinguish "body field" from
/// "typo" by name alone.
fn partition_call_params(
    ep: Option<&Endpoint>,
    param: &[String],
) -> Result<PartitionedParams, CliCoreError> {
    let mut parts = PartitionedParams::default();
    for raw in param {
        let (name, value) = split_kv(raw).ok_or_else(|| {
            crate::error::GddyError::validation(format!(
                "invalid --param '{raw}': expected name=value"
            ))
            .into_cli_error()
        })?;
        let location = ep.and_then(|ep| {
            ep.parameters
                .iter()
                .find(|p| p.get("name").and_then(Value::as_str) == Some(name))
                .or_else(|| {
                    // HTTP header names are case-insensitive (RFC 9110), so
                    // e.g. `--param idempotency-key=...` should still match
                    // a declared `Idempotency-Key` header/cookie parameter,
                    // even though path/query names are exact-case.
                    ep.parameters.iter().find(|p| {
                        matches!(
                            p.get("in").and_then(Value::as_str),
                            Some("header" | "cookie")
                        ) && p
                            .get("name")
                            .and_then(Value::as_str)
                            .is_some_and(|n| n.eq_ignore_ascii_case(name))
                    })
                })
                .and_then(|p| p.get("in").and_then(Value::as_str))
        });
        match location {
            Some("path") => parts.path.push((name.to_owned(), value.to_owned())),
            Some("query") => parts.query.push((name.to_owned(), value.to_owned())),
            Some("header" | "cookie") => parts.header.push((name.to_owned(), value.to_owned())),
            _ => parts.body.push((name.to_owned(), value.to_owned())),
        }
    }
    Ok(parts)
}

/// Query parameter names already present in a literal path's own `?...`
/// suffix (e.g. `/v1/foo?limit=10`) — the literal-path analog of a path
/// template's `{name}` placeholder already being filled in with a concrete
/// value: the caller has already supplied it, so required-value validation
/// shouldn't ask for it again via `--param`.
fn existing_query_param_names(path: &str) -> Vec<&str> {
    let Some((_, query)) = path.split_once('?') else {
        return Vec::new();
    };
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| pair.split('=').next().unwrap_or(pair))
        .collect()
}

/// Every parameter `ep` declares required (any location) must be satisfied
/// by something — a routed `--param` value; for `path`, its `{name}`
/// placeholder no longer being present in `path` (substituted by `--param`,
/// or the caller already passed a literal path with the value filled in);
/// for `query`, the name already present in `path`'s own `?...` suffix; for
/// `header`/`cookie`, a matching entry in `headers` (from `--header` or a
/// routed `--param`). Batches every unsatisfied name into one error, mirroring
/// `build_graphql_call`'s missing-required check (`graphql/call.rs`). Body
/// location isn't enforced — same as `--field` never validating the body
/// against a schema.
fn validate_required_params(
    ep: &Endpoint,
    path: &str,
    parts: &PartitionedParams,
    headers: &[(String, String)],
) -> Result<(), CliCoreError> {
    let existing_query = existing_query_param_names(path);
    let missing: Vec<&str> = ep
        .parameters
        .iter()
        .filter(|p| p.get("required").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|p| {
            let name = p.get("name").and_then(Value::as_str)?;
            let satisfied = match p.get("in").and_then(Value::as_str) {
                Some("path") => !path.contains(&format!("{{{name}}}")),
                Some("query") => {
                    parts.query.iter().any(|(n, _)| n == name) || existing_query.contains(&name)
                }
                Some("header" | "cookie") => {
                    headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(name))
                }
                _ => true,
            };
            (!satisfied).then_some(name)
        })
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(crate::error::GddyError::validation(format!(
        "missing required --param value(s) for {}: {}",
        ep.operation_id,
        missing.join(", "),
    ))
    .with_fix(format!("Run: gddy api operation get {}", ep.operation_id))
    .into_cli_error())
}

/// Appends `query` to `url` as URL-encoded query-string pairs. A no-op (and
/// no parse attempt) when `query` is empty, so an unmatched/query-param-free
/// call never risks failing on an otherwise-valid `url`.
fn append_query(url: &str, query: &[(String, String)]) -> Result<String, CliCoreError> {
    if query.is_empty() {
        return Ok(url.to_owned());
    }
    let mut parsed = url::Url::parse(url).map_err(|e| {
        crate::error::GddyError::validation(format!("invalid URL '{url}': {e}")).into_cli_error()
    })?;
    {
        let mut qp = parsed.query_pairs_mut();
        for (k, v) in query {
            qp.append_pair(k, v);
        }
    }
    Ok(parsed.to_string())
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig};

    use super::{
        Endpoint, PartitionedParams, append_query, partition_call_params, validate_required_params,
    };

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

    /// A bare operation id with no catalog match must be rejected with a
    /// clear not-found error from `resolve_operation`, not silently built
    /// into an invalid request URL.
    #[tokio::test]
    async fn call_rejects_an_unresolvable_operation_id() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_module(super::super::module()),
        );
        let output = cli
            .run([
                "gddy",
                "api",
                "call",
                "totallyMadeUpOperationId",
                "--dry-run",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            output.rendered.contains("no operation found matching"),
            "{}",
            output.rendered
        );
    }

    /// A bare operation id resolves via `resolve_operation`, substituting
    /// its declared path and header parameters from `--param` — exercised via
    /// a mutating method so it's safe to run under `--dry-run` without a
    /// token, and the preview reports the resolved concrete path.
    #[tokio::test]
    async fn call_dry_run_resolves_operation_id_and_substitutes_params() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_module(super::super::module()),
        );
        let output = cli
            .run([
                "gddy",
                "api",
                "call",
                "updateNameservers",
                "--method",
                "PUT",
                "--param",
                "domain-name=example.com",
                "--param",
                "Idempotency-Key=11111111-1111-1111-1111-111111111111",
                "--dry-run",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(
            rendered["data"]["path"], "/v3/domains/domain-names/example.com/nameservers",
            "{}",
            output.rendered
        );
    }

    /// Missing a required `--param` for a resolved operation id (here a
    /// required header, not a path param) is a validation error naming it,
    /// not a live request attempt.
    #[tokio::test]
    async fn call_rejects_a_resolved_operation_missing_a_required_param() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_module(super::super::module()),
        );
        let output = cli
            .run([
                "gddy",
                "api",
                "call",
                "updateNameservers",
                "--method",
                "PUT",
                "--param",
                "domain-name=example.com",
                "--dry-run",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            output
                .rendered
                .contains("missing required --param value(s)"),
            "{}",
            output.rendered
        );
        assert!(
            output.rendered.contains("Idempotency-Key"),
            "{}",
            output.rendered
        );
    }

    /// A literal path processes identically to the same operation located
    /// by id: it resolves via `locate_by_path`, the header `--param` routes
    /// the same way, and the preview reports the same resolved `path` —
    /// except the path parameter needs no `--param` at all, since the literal
    /// path already has it filled in.
    #[tokio::test]
    async fn call_dry_run_literal_path_processes_the_same_as_operation_id() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_module(super::super::module()),
        );
        let output = cli
            .run([
                "gddy",
                "api",
                "call",
                "/v3/domains/domain-names/example.com/nameservers",
                "--method",
                "PUT",
                "--param",
                "Idempotency-Key=11111111-1111-1111-1111-111111111111",
                "--dry-run",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(
            rendered["data"]["path"], "/v3/domains/domain-names/example.com/nameservers",
            "{}",
            output.rendered
        );
    }

    /// A literal path with its own `?...` query string still resolves via
    /// `locate_by_path` (which strips it before matching against templated
    /// catalog paths) — an unmatched fallback would silently skip
    /// required-value validation, so this exercises the regression by
    /// confirming a still-missing required header is still caught.
    #[tokio::test]
    async fn call_dry_run_literal_path_with_query_string_still_resolves() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_module(super::super::module()),
        );
        let output = cli
            .run([
                "gddy",
                "api",
                "call",
                "/v3/domains/domain-names/example.com/nameservers?foo=bar",
                "--method",
                "PUT",
                "--dry-run",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            output
                .rendered
                .contains("missing required --param value(s)"),
            "{}",
            output.rendered
        );
        assert!(
            output.rendered.contains("Idempotency-Key"),
            "{}",
            output.rendered
        );
    }

    /// An unmatched literal path has no schema to route `--param` against, so
    /// every value falls through to the body — same permissiveness as
    /// `--field`, and the same thing that happens for an unrecognized
    /// `--param` name on a matched endpoint.
    #[tokio::test]
    async fn call_dry_run_unmatched_literal_path_param_falls_through_to_body() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_module(super::super::module()),
        );
        let output = cli
            .run([
                "gddy",
                "api",
                "call",
                "/v1/totally/unmatched/path",
                "--method",
                "POST",
                "--param",
                "note=hello",
                "--dry-run",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
    }

    fn endpoint_fixture(json: &str) -> Endpoint {
        serde_json::from_str(json).expect("valid endpoint fixture")
    }

    #[test]
    fn partition_call_params_routes_by_declared_location() {
        let ep = endpoint_fixture(
            r#"{
                "operationId": "testOp",
                "method": "POST",
                "path": "/things/{thingId}",
                "summary": "",
                "parameters": [
                    {"name": "thingId", "in": "path", "required": true},
                    {"name": "filter", "in": "query", "required": false},
                    {"name": "x-trace-id", "in": "header", "required": false}
                ]
            }"#,
        );
        let parts = partition_call_params(
            Some(&ep),
            &[
                "thingId=abc".to_owned(),
                "filter=active".to_owned(),
                "x-trace-id=t1".to_owned(),
                "note=hello".to_owned(),
            ],
        )
        .expect("valid args");
        assert_eq!(parts.path, vec![("thingId".to_owned(), "abc".to_owned())]);
        assert_eq!(
            parts.query,
            vec![("filter".to_owned(), "active".to_owned())]
        );
        assert_eq!(
            parts.header,
            vec![("x-trace-id".to_owned(), "t1".to_owned())]
        );
        assert_eq!(parts.body, vec![("note".to_owned(), "hello".to_owned())]);
    }

    /// HTTP header names are case-insensitive (RFC 9110) — a `--param` with
    /// different casing than the declared header parameter must still route
    /// to `header`, not fall through to the body.
    #[test]
    fn partition_call_params_routes_a_header_case_insensitively() {
        let ep = endpoint_fixture(
            r#"{
                "operationId": "testOp",
                "method": "PUT",
                "path": "/things",
                "summary": "",
                "parameters": [
                    {"name": "Idempotency-Key", "in": "header", "required": true}
                ]
            }"#,
        );
        let parts = partition_call_params(Some(&ep), &["idempotency-key=abc".to_owned()])
            .expect("valid args");
        assert!(parts.body.is_empty(), "must not fall through to the body");
        assert_eq!(
            parts.header,
            vec![("idempotency-key".to_owned(), "abc".to_owned())]
        );
    }

    /// No matched endpoint (e.g. an unmatched literal path) means no
    /// declared parameters to route against — every `--param` falls through
    /// to the body, same permissiveness as `--field`.
    #[test]
    fn partition_call_params_with_no_endpoint_routes_everything_to_body() {
        let parts =
            partition_call_params(None, &["a=1".to_owned(), "b=2".to_owned()]).expect("valid args");
        assert!(parts.path.is_empty());
        assert!(parts.query.is_empty());
        assert!(parts.header.is_empty());
        assert_eq!(
            parts.body,
            vec![
                ("a".to_owned(), "1".to_owned()),
                ("b".to_owned(), "2".to_owned())
            ]
        );
    }

    #[test]
    fn partition_call_params_rejects_a_malformed_pair() {
        let ep = endpoint_fixture(
            r#"{"operationId": "testOp", "method": "GET", "path": "/things", "summary": ""}"#,
        );
        let err = partition_call_params(Some(&ep), &["no-equals-sign".to_owned()])
            .expect_err("must reject a value with no '='");
        assert!(err.to_string().contains("expected name=value"), "{err}");
    }

    #[test]
    fn validate_required_params_batches_every_missing_name() {
        let ep = endpoint_fixture(
            r#"{
                "operationId": "testOp",
                "method": "POST",
                "path": "/things/{thingId}",
                "summary": "",
                "parameters": [
                    {"name": "thingId", "in": "path", "required": true},
                    {"name": "x-trace-id", "in": "header", "required": true}
                ]
            }"#,
        );
        let err =
            validate_required_params(&ep, &ep.path.clone(), &PartitionedParams::default(), &[])
                .expect_err("both required params are missing");
        let msg = err.to_string();
        assert!(msg.contains("thingId"), "{msg}");
        assert!(msg.contains("x-trace-id"), "{msg}");
    }

    /// A literal path that already has a required path param's value
    /// filled in (no `{name}` placeholder left) satisfies it without any
    /// `--param` — the literal-path analog of substituting one in.
    #[test]
    fn validate_required_params_path_satisfied_by_an_already_filled_in_literal_path() {
        let ep = endpoint_fixture(
            r#"{
                "operationId": "testOp",
                "method": "PUT",
                "path": "/things/{thingId}",
                "summary": "",
                "parameters": [
                    {"name": "thingId", "in": "path", "required": true}
                ]
            }"#,
        );
        validate_required_params(&ep, "/things/abc123", &PartitionedParams::default(), &[])
            .expect("thingId's placeholder is already filled in, no --param needed");
    }

    /// A required query param already present in a literal path's own
    /// `?...` suffix satisfies it without `--param` — otherwise a caller who
    /// already fully specifies their own query string would be newly
    /// rejected by validation that didn't exist before `--param` did.
    #[test]
    fn validate_required_params_query_satisfied_by_an_existing_query_string() {
        let ep = endpoint_fixture(
            r#"{
                "operationId": "testOp",
                "method": "GET",
                "path": "/things",
                "summary": "",
                "parameters": [
                    {"name": "filter", "in": "query", "required": true}
                ]
            }"#,
        );
        validate_required_params(
            &ep,
            "/things?filter=active",
            &PartitionedParams::default(),
            &[],
        )
        .expect("filter is already present in the literal path's query string");
    }

    /// A required header satisfied by a plain `--header` flag (not routed
    /// through `--param`) counts — `headers` is the merged `--header` +
    /// `--param`-routed-header list, so either channel satisfies it.
    #[test]
    fn validate_required_params_header_satisfied_by_a_header_flag() {
        let ep = endpoint_fixture(
            r#"{
                "operationId": "testOp",
                "method": "PUT",
                "path": "/things",
                "summary": "",
                "parameters": [
                    {"name": "Idempotency-Key", "in": "header", "required": true}
                ]
            }"#,
        );
        let headers = vec![("Idempotency-Key".to_owned(), "abc".to_owned())];
        validate_required_params(&ep, "/things", &PartitionedParams::default(), &headers)
            .expect("Idempotency-Key was supplied via a plain --header flag");
    }

    /// Header names are case-insensitive (RFC 9110) — a required
    /// `Idempotency-Key` is satisfied by a differently-cased `--header`
    /// entry too, not just an exact-case match.
    #[test]
    fn validate_required_params_header_satisfied_case_insensitively() {
        let ep = endpoint_fixture(
            r#"{
                "operationId": "testOp",
                "method": "PUT",
                "path": "/things",
                "summary": "",
                "parameters": [
                    {"name": "Idempotency-Key", "in": "header", "required": true}
                ]
            }"#,
        );
        let headers = vec![("idempotency-key".to_owned(), "abc".to_owned())];
        validate_required_params(&ep, "/things", &PartitionedParams::default(), &headers)
            .expect("differently-cased header name still satisfies the requirement");
    }

    #[test]
    fn append_query_is_a_no_op_for_no_query_params() {
        assert_eq!(
            append_query("https://example.com/foo", &[]).expect("no-op"),
            "https://example.com/foo"
        );
    }

    #[test]
    fn append_query_encodes_multiple_pairs() {
        let url = append_query(
            "https://example.com/foo",
            &[
                ("a".to_owned(), "1".to_owned()),
                ("b".to_owned(), "hello world".to_owned()),
            ],
        )
        .expect("valid url");
        assert_eq!(url, "https://example.com/foo?a=1&b=hello+world");
    }
}
