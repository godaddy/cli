//! `api operation list` and `api operation get`.

use cli_engine::{
    CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, TableColumn, Tier,
};
use serde_json::{Value, json};

use crate::next_action::{next_action, required_value};
use crate::output_schema::output_schema;
use crate::summary::Summary;

use super::catalog::{
    catalog, graphql_operation_redirect_error, graphql_sub_endpoint_rows,
    resolve_graphql_operation, resolve_operation,
};
use super::summary::{
    OPERATION_CHILD_LIST_DEFAULT_LIMIT, response_rows, summarize_graphql_schema,
    summarize_parameters,
};

// `api operation list --domain X` lists endpoints within one domain, so each row
// omits the (redundant) domain field that the cross-domain `api search` emits.
output_schema!(ApiDomainEndpoint {
    "operationId": "string";
    "method": "string";
    "path": "string";
    "summary": "string", optional;
    "scopes": "[]string";
    "graphqlOperations": "number", optional;
    // Present only on a synthetic row for one addressable GraphQL
    // query/mutation (`<parentOperationId>::<query|mutation>::<name>`) —
    // `"query"` or `"mutation"`. Absent for a real REST/wrapper row.
    "kind": "string", optional;
});

output_schema!(ApiOperation {
    "domain": "string";
    "baseUrl": "string";
    "operationId": "string";
    "method": "string";
    "path": "string";
    "fullPath": "string";
    "summary": "string", optional;
    "description": "string", optional;
    "parameters": "object";
    "responses": "object";
    "scopes": "[]string";
    "graphql": "object", optional;
    "message": "string", optional;
    "matches": "[]object", optional;
});

#[derive(Debug, Clone, clap::Args)]
struct OperationListArgs {
    /// API domain whose endpoints to list (see `api domain list`).
    #[arg(long, value_name = "DOMAIN")]
    domain: String,
}

pub(super) fn list_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed::<OperationListArgs, _, _, _>(
        CommandSpec::from_args::<OperationListArgs>("list", "List operations within an API domain")
            .with_long(
                "Lists operations in one API domain. Use `api domain list` to \
                find available domain names, `api operation get <operationId>` \
                to view full details, and `api call <path>` to execute a request.",
            )
            .with_system("api")
            .with_tier(Tier::Read)
            .no_auth(true)
            .with_default_fields("operationId,method,path,summary")
            .with_output_schema::<ApiDomainEndpoint>(),
        |_cred, args: OperationListArgs| async move {
            let catalog = catalog();
            let domain_filter = args.domain.as_str();
            let domain = catalog
                .iter()
                .find(|d| d.name == domain_filter)
                .ok_or_else(|| {
                    crate::error::GddyError::not_found(format!(
                        "domain '{domain_filter}' not found"
                    ))
                    .with_fix("Run: gddy api domain list")
                    .into_cli_error()
                })?;
            let mut endpoints: Vec<Value> = domain
                .endpoints
                .iter()
                .map(|ep| {
                    json!({
                        "operationId": ep.operation_id,
                        "method": ep.method,
                        "path": ep.path,
                        "summary": ep.summary,
                        "scopes": ep.scopes,
                        "graphqlOperations": ep.graphql.as_ref().map(|g| g.operation_count),
                    })
                })
                .collect();
            // Every GraphQL query/mutation field is just as addressable as a
            // REST operation — surface it as its own row alongside the
            // wrapper's, tagged with `kind` so the two are easy to tell
            // apart at a glance.
            let graphql_wrappers: Vec<&str> = domain
                .endpoints
                .iter()
                .filter(|ep| ep.graphql.is_some())
                .map(|ep| ep.operation_id.as_str())
                .collect();
            for ep in &domain.endpoints {
                endpoints.extend(graphql_sub_endpoint_rows(None, ep));
            }

            let mut next_actions = vec![
                next_action(
                    "api operation get <operation>",
                    "Get full details for an operation",
                )
                .with_param("operation", NextActionParam::required()),
            ];
            if !graphql_wrappers.is_empty() {
                next_actions.push(
                    next_action(
                        "api graphql get <operation>",
                        "See a GraphQL operation's own shape (arguments and real return type)",
                    )
                    .with_param("operation", NextActionParam::required()),
                );
                for wrapper in &graphql_wrappers {
                    next_actions.push(next_action(
                        format!("api graphql sdl get {wrapper}"),
                        format!("See {wrapper}'s actual GraphQL schema (SDL) text, verbatim"),
                    ));
                }
            }

            Ok(CommandResult::new(json!(endpoints)).with_next_actions(next_actions))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct OperationGetArgs {
    /// Operation ID (e.g. createOrder) or path fragment (e.g. /v1/commerce/orders).
    #[arg(value_name = "OPERATION")]
    operation: String,

    /// Filter to a specific HTTP method (GET, POST, PUT, PATCH, DELETE).
    #[arg(long, short = 'm', value_name = "METHOD")]
    method: Option<String>,
}

pub(super) fn get_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<OperationGetArgs, _, _, _>(
        CommandSpec::from_args::<OperationGetArgs>(
            "get",
            "Show schema and parameters for an operation",
        )
        .with_long(
            "Shows the full details of one API operation: HTTP method, path, required and \
             optional parameters, request body schema, response shapes, and declared OAuth \
             scopes. Accepts an operation ID (e.g. createOrder) or a path fragment \
             (e.g. /v1/commerce/orders). No authentication is required.",
        )
        .with_system("api")
        .with_tier(Tier::Read)
        .no_auth(true)
        .with_output_schema::<ApiOperation>()
        .with_view(vec![
            TableColumn::new("domain", "Domain"),
            TableColumn::new("operationId", "Operation ID"),
            TableColumn::new("method", "Method"),
            TableColumn::new("path", "Path"),
            TableColumn::new("summary", "Summary"),
            TableColumn::new("parameters.items", "Parameters").nested(vec![
                TableColumn::new("name", "Name"),
                TableColumn::new("in", "In"),
                TableColumn::new("required", "Required"),
                TableColumn::new("type", "Type"),
                TableColumn::new("schemaId", "Schema ID"),
                TableColumn::new("description", "Description"),
            ]),
            TableColumn::new("responses.items", "Responses").nested(vec![
                TableColumn::new("status", "Status"),
                TableColumn::new("type", "Type"),
                TableColumn::new("schemaId", "Schema ID"),
                TableColumn::new("description", "Description"),
            ]),
        ]),
        |ctx, args: OperationGetArgs| async move {
            let query = args.operation.as_str();
            let method_filter = args.method.map(|m| m.to_uppercase());
            let catalog = catalog();

            // A GraphQL operation id (`<parent>::<kind>::<name>`) isn't a
            // valid `operation get` target at all — it has its own
            // dedicated command (`api graphql get`) with an output shape
            // that doesn't try to fit GraphQL semantics into REST-operation
            // columns.
            if resolve_graphql_operation(catalog, query).is_some() {
                return Err(graphql_operation_redirect_error(query, "graphql get"));
            }

            // Exact operationId/path/template match (optionally narrowed by
            // --method) first; a miss falls back to fuzzy substring search,
            // which ignores --method entirely (matches the original CLI).
            // Either step can legitimately produce more than one candidate
            // (e.g. GET/POST sharing a path with no --method given) — both
            // are treated as the same kind of ambiguity.
            let (domain, ep) = resolve_operation(catalog, query, method_filter.as_deref())?;

            // Env-aware base URL, matching `domain_list_command`/`call_command`
            // — the catalog's `baseUrl` is a static, prod-shaped value; this
            // resolves it against the active environment the same way an
            // actual `api call` to this endpoint would.
            let base_url = crate::environments::resolve_catalog_base_url(
                &domain.name,
                &domain.base_url,
                &ctx.middleware.env,
            );

            // Summarized and capped, not inlined in full — a truncated
            // preview here links to the standalone `parameter list`/
            // `response list` (which do their own, independent truncation)
            // rather than dumping everything inline.
            let param_summary = Summary::capped(
                summarize_parameters(ep, &domain.defs),
                OPERATION_CHILD_LIST_DEFAULT_LIMIT,
            );
            let response_summary = Summary::capped(
                response_rows(ep, &domain.defs),
                OPERATION_CHILD_LIST_DEFAULT_LIMIT,
            );

            // Strip `base_url`'s scheme+host generically (not a hard-coded
            // prod hostname) so `fullPath` stays a hostless path prefix +
            // endpoint path consistently across every environment, not just
            // prod — `resolve_catalog_base_url` rewrites the host per env
            // (e.g. `api.ote-godaddy.com`), which a literal prod-host strip
            // would silently leave un-stripped.
            let full_path = {
                let without_scheme = base_url
                    .split_once("://")
                    .map_or(base_url.as_str(), |(_, rest)| rest);
                let path_prefix = without_scheme
                    .find('/')
                    .map_or("", |i| &without_scheme[i..]);
                format!("{path_prefix}{}", ep.path)
            };

            let mut next_actions = vec![next_action(
                format!("api call {} --method {}", ep.path, ep.method),
                "Make an authenticated call to this endpoint",
            )];
            next_actions.extend(
                param_summary.next_action_if_truncated(
                    next_action(
                        "api parameter list --operation <operation>",
                        "See all parameters",
                    )
                    .with_param("operation", required_value(ep.operation_id.clone())),
                ),
            );
            next_actions.extend(
                response_summary.next_action_if_truncated(
                    next_action(
                        "api response list --operation <operation>",
                        "See all responses",
                    )
                    .with_param("operation", required_value(ep.operation_id.clone())),
                ),
            );
            // A schema id shown in the table but not echoed anywhere in
            // `next_actions` is a dead end unless the caller already knows
            // `api schema get <id>` exists. One generic pointer to that
            // command covers every parameter/response with a `schemaId` at
            // once, rather than a same-command line repeated per row (most
            // schema ids on one operation are typically shared, e.g. a
            // common `Error` response schema across several status codes).
            if param_summary
                .items
                .iter()
                .any(|param| param.schema_id.is_some())
                || response_summary
                    .items
                    .iter()
                    .any(|resp| resp.schema_id.is_some())
            {
                next_actions.push(
                    next_action(
                        "api schema get <id>",
                        "See a parameter's or response's full schema — use its Schema ID from the table above",
                    )
                    .with_param("id", NextActionParam::required()),
                );
            }

            Ok(CommandResult::new(json!({
                "domain": domain.name,
                "baseUrl": base_url,
                "operationId": ep.operation_id,
                "method": ep.method,
                "path": ep.path,
                "fullPath": full_path,
                "summary": ep.summary,
                "description": ep.description,
                "parameters": param_summary,
                "responses": response_summary,
                "scopes": ep.scopes,
                "graphql": ep.graphql.as_ref().map(summarize_graphql_schema),
            }))
            .with_next_actions(next_actions))
        },
    )
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig};
    use serde_json::json;

    /// `resolve_catalog_base_url` needs `ctx.middleware.env` to actually
    /// resolve to `DEFAULT_ENV` ("prod") rather than an empty default, so
    /// wire environments the same way `main.rs` does for the real CLI.
    ///
    /// Each command file with an end-to-end `Cli`-driven test defines this
    /// same small fixture locally rather than sharing one across sibling
    /// modules — it's a handful of lines, and keeps each file's test module
    /// self-contained.
    fn operation_cli() -> Cli {
        Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_module(super::super::module())
                .with_environments(std::sync::Arc::clone(crate::environments::instance())),
        )
    }

    #[tokio::test]
    async fn operation_get_exact_operation_id_match() {
        // Pin `--env` explicitly: `fullPath` depends on env-resolved base
        // URL, and the default env is read from ambient local config
        // (`gdenv`), so a bare run isn't hermetic across machines/CI.
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "commerce.location.verify-address",
                "--env",
                "prod",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(
            rendered["data"]["operationId"],
            json!("commerce.location.verify-address")
        );
        assert_eq!(
            rendered["data"]["fullPath"],
            json!("/v1/commerce/location/address-verifications")
        );
    }

    /// `fullPath` must stay a hostless path in every environment, not just
    /// prod — `resolve_catalog_base_url` rewrites the host for non-prod envs
    /// (e.g. `api.ote-godaddy.com`), so stripping only the literal prod host
    /// would silently leave the scheme+host in place here.
    #[tokio::test]
    async fn operation_get_full_path_is_hostless_in_a_non_prod_env() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "commerce.location.verify-address",
                "--env",
                "ote",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(
            rendered["data"]["baseUrl"],
            json!("https://api.ote-godaddy.com/v1/commerce")
        );
        assert_eq!(
            rendered["data"]["fullPath"],
            json!("/v1/commerce/location/address-verifications")
        );
    }

    #[tokio::test]
    async fn operation_get_exact_match_narrowed_by_method_flag() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "/location/addresses",
                "--method",
                "GET",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(
            rendered["data"]["operationId"],
            json!("commerce.location.search-addresses")
        );
    }

    /// Matches the original CLI's documented quirk: `--method` only narrows
    /// the exact-match step. A mismatched method falls through to fuzzy
    /// search, which ignores the method filter entirely — so this still
    /// resolves to the (wrong-method) endpoint rather than erroring.
    #[tokio::test]
    async fn operation_get_method_filter_is_ignored_during_fuzzy_fallback() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "/location/addresses",
                "--method",
                "POST",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(
            rendered["data"]["operationId"],
            json!("commerce.location.search-addresses")
        );
        assert_eq!(rendered["data"]["method"], json!("GET"));
    }

    #[tokio::test]
    async fn operation_get_single_fuzzy_match_resolves_transparently() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "verify-address",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(
            rendered["data"]["operationId"],
            json!("commerce.location.verify-address")
        );
    }

    /// A fuzzy query matching several unrelated endpoints is a hard error
    /// (see `resolve_operation`/`ambiguous_operation_error`), not a
    /// success-shaped `{message, matches}` response — cli-engine's error
    /// envelope has no structured `next_actions` hook, so every candidate
    /// is instead a runnable command line inside the error's `fix` string.
    #[tokio::test]
    async fn operation_get_multiple_fuzzy_matches_is_an_ambiguous_error() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "/location",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["error"]["code"], json!("AMBIGUOUS_MATCH"));
        assert!(
            rendered["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains("matches 2 operations"),
            "{}",
            output.rendered
        );
        let fix = rendered["fix"].as_str().expect("fix present");
        assert!(fix.contains("/location/address-verifications"), "{fix}");
        assert!(fix.contains("/location/addresses"), "{fix}");
        assert!(fix.contains("gddy api operation get"), "{fix}");
    }

    /// Same ambiguity, but through human output — covers that rendering
    /// path directly, since every other test here only exercises
    /// `--output json`. Human error rendering is `Error: {message}` /
    /// `Fix: {fix}` (`cli_engine::output::human::render_human_with_view`).
    #[tokio::test]
    async fn operation_get_multiple_fuzzy_matches_human_output_shows_message_and_candidates() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "/location",
                "--output",
                "human",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            output.rendered.contains("'/location' matches 2 operations"),
            "{}",
            output.rendered
        );
        assert!(
            output.rendered.contains("/location/address-verifications"),
            "{}",
            output.rendered
        );
        assert!(
            output.rendered.contains("/location/addresses"),
            "{}",
            output.rendered
        );
    }

    /// Many catalog paths are shared by several endpoints that only differ
    /// by method (here: `GET /businesses` and `POST /businesses`) — without
    /// `--method`, exact-match resolution must treat that the same as an
    /// ambiguous fuzzy match, not silently pick whichever one it saw first.
    #[tokio::test]
    async fn operation_get_exact_path_shared_by_multiple_methods_is_ambiguous() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "/businesses",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["error"]["code"], json!("AMBIGUOUS_MATCH"));
        // Candidates sharing one path are only distinguishable by method, so
        // the fix must spell out both.
        let fix = rendered["fix"].as_str().expect("fix present");
        assert!(fix.contains("--method GET"), "{fix}");
        assert!(fix.contains("--method POST"), "{fix}");
    }

    /// The same ambiguity, resolved by adding `--method`.
    #[tokio::test]
    async fn operation_get_exact_path_shared_by_multiple_methods_resolves_with_method_flag() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "/businesses",
                "--method",
                "POST",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"]["operationId"], json!("createBusiness"));
    }

    /// A concrete path with a real ID substituted for a `{param}` segment
    /// must resolve via template matching, same as `api call` already does
    /// via `find_endpoint`/`path_matches_template`.
    #[tokio::test]
    async fn operation_get_concrete_path_resolves_via_template_matching() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "/businesses/abc123",
                "--method",
                "GET",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"]["operationId"], json!("getBusinessById"));
    }

    #[tokio::test]
    async fn operation_get_zero_matches_is_an_error() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "totally-fake-endpoint-xyz",
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

    /// Regression: `.with_view(...)` covered `parameters.items` but had no
    /// `responses.items` column at all, so `responses` — present in the
    /// JSON the whole time — silently never rendered in `--output human`,
    /// with no error or missing-column notice to say so.
    #[tokio::test]
    async fn operation_get_human_output_shows_a_responses_table() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "createShipment",
                "--output",
                "human",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            output.rendered.contains("Responses:"),
            "{}",
            output.rendered
        );
        assert!(output.rendered.contains("STATUS"), "{}", output.rendered);
        assert!(output.rendered.contains("200"), "{}", output.rendered);
    }

    /// Most catalog request bodies are a bare `$ref` with no inline
    /// `properties` (63/82 in the embedded catalog) — `createBusiness`'s is
    /// `{"$ref": "#/$defs/Business"}`. Without $ref resolution this
    /// summarizes to `null`; with it, the real fields show up. The request
    /// body is folded into `parameters` as the synthetic `body` row.
    #[tokio::test]
    async fn operation_get_resolves_a_pure_ref_request_body_against_defs() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "createBusiness",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        let parameters = rendered["data"]["parameters"]["items"]
            .as_array()
            .expect("parameters items array");
        let body = parameters
            .iter()
            .find(|p| p["name"] == "body")
            .expect("synthetic body row");
        let schema = body["schema"]["items"]
            .as_array()
            .expect("body.schema resolves to a property list, not null");
        let active_since = schema
            .iter()
            .find(|p| p["name"] == "activeSince")
            .expect("activeSince property");
        assert_eq!(active_since["type"], json!("string(date-time)"));
        assert_eq!(body["schemaId"], json!("Business"));
    }

    /// `getChannels` has 6 parameters — under the preview cap, so its
    /// `operation get` output isn't truncated and shouldn't link to the
    /// standalone `parameter list`.
    #[tokio::test]
    async fn operation_get_does_not_link_to_parameter_list_when_untruncated() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "getChannels",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(
            rendered["data"]["parameters"]["pagination"]["total"],
            json!(6)
        );
        assert!(
            !rendered["data"]["parameters"]["pagination"]["has_more"]
                .as_bool()
                .unwrap_or(true)
        );
        let next_actions = rendered["next_actions"].as_array().expect("next_actions");
        assert!(
            next_actions
                .iter()
                .any(|a| a["command"].as_str().unwrap_or("").contains("api call")),
            "{}",
            output.rendered
        );
        assert!(
            !next_actions.iter().any(|a| a["command"]
                .as_str()
                .unwrap_or("")
                .contains("parameter list")),
            "an untruncated preview should not link to the standalone list: {}",
            output.rendered
        );
    }

    /// `createShipment`'s synthetic `body` parameter has a `schemaId`
    /// (`Shipment`, a `$ref`) — a caller needs to be told `api schema get`
    /// exists at all to make use of it, or the id in the table is a dead
    /// end. One generic hint covers every parameter/response with a
    /// `schemaId`, rather than a same-command line repeated per row.
    #[tokio::test]
    async fn operation_get_links_to_schema_get_when_any_row_has_a_schema_id() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "createShipment",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        let next_actions = rendered["next_actions"].as_array().expect("next_actions");
        let hint = next_actions
            .iter()
            .find(|a| a["command"] == json!("gddy api schema get <id>"))
            .expect("generic schema-get hint");
        assert!(hint["params"]["id"]["required"].as_bool().unwrap_or(false));
        assert!(hint["params"]["id"]["value"].is_null());
    }

    /// `deleteDNSRecord` has no schema anywhere (every parameter is a bare
    /// scalar, and its one response is a bare `204` with no body) — the
    /// schema-get hint must not appear when there's nothing for it to point
    /// at.
    #[tokio::test]
    async fn operation_get_does_not_link_to_schema_get_when_no_row_has_a_schema_id() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "deleteDNSRecord",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        let next_actions = rendered["next_actions"].as_array().expect("next_actions");
        assert!(
            !next_actions
                .iter()
                .any(|a| a["command"] == json!("gddy api schema get <id>")),
            "{}",
            output.rendered
        );
    }

    /// `get_transaction_disputes` has 26 parameters — over the preview cap
    /// — so its `operation get` output must be truncated and link to the
    /// standalone `parameter list` for the rest, per the "list truncated →
    /// standalone list command" convention.
    #[tokio::test]
    async fn operation_get_links_to_parameter_list_when_truncated() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "get_transaction_disputes",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        // `Summary`'s only field beyond `items` is `pagination` (a
        // `PaginationMeta`) — the same shape cli-engine's own top-level
        // pagination uses, so cli-engine 0.8.2 (DEVEX-985) can read it as a
        // sibling of a nested column's array and render the same "(N of M
        // rows, offset O, limit L)" footer a top-level paginated array gets.
        assert_eq!(
            rendered["data"]["parameters"]["pagination"]["total"],
            json!(26)
        );
        assert_eq!(
            rendered["data"]["parameters"]["pagination"]["count"],
            json!(20)
        );
        assert_eq!(
            rendered["data"]["parameters"]["pagination"]["limit"],
            json!(20)
        );
        assert_eq!(
            rendered["data"]["parameters"]["pagination"]["has_more"],
            json!(true)
        );
        let next_actions = rendered["next_actions"].as_array().expect("next_actions");
        let link = next_actions
            .iter()
            .find(|a| {
                a["command"]
                    .as_str()
                    .unwrap_or("")
                    .contains("parameter list")
            })
            .expect("next_action linking to the standalone parameter list");
        assert_eq!(
            link["params"]["operation"]["value"],
            json!("get_transaction_disputes")
        );
    }

    /// Same fixture as `operation_get_links_to_parameter_list_when_truncated`,
    /// through human output: DEVEX-985 (cli-engine 0.8.2) lets a nested
    /// table render a truncation footer at all, so this is the first time
    /// `--output human` can show "not all rows are here" for `operation
    /// get`'s embedded parameter preview, rather than a bare `(20 rows)`
    /// with no hint that 6 more exist.
    #[tokio::test]
    async fn operation_get_human_output_shows_truncation_footer_for_parameters() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "get_transaction_disputes",
                "--output",
                "human",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            output
                .rendered
                .contains("(20 of 26 rows, offset 0, limit 20)"),
            "{}",
            output.rendered
        );
    }

    #[tokio::test]
    async fn operation_get_graphql_endpoint_gets_summary() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "postCatalogGraphql",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        let data = &rendered["data"];
        let operation_count = data["graphql"]["operationCount"]
            .as_u64()
            .expect("operationCount is a number");
        assert!(operation_count > 100, "{operation_count}");
        assert_eq!(
            data["graphql"]["operations"]
                .as_array()
                .expect("operations array")
                .len() as u64,
            operation_count
        );
    }
}
