//! `api graphql call`.

use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use serde_json::{Map, Value, json};

use super::super::catalog::{
    GraphqlOpRef, catalog, graphql_base_type_name, graphql_operation_id,
    graphql_operation_not_found_error, graphql_valid_arg_names, resolve_graphql_operation,
};
use super::super::http::{merge_required_scopes, parsed_extra_headers, send_and_report};

#[derive(Debug, Clone, clap::Args)]
struct GraphqlCallArgs {
    /// GraphQL operation id (see `api operation list --domain <domain>` or `api search`).
    #[arg(value_name = "ID")]
    id: String,

    /// GraphQL variable or wrapper call-requirement value (name=value,
    /// repeatable) — see `api graphql get <id>` for valid names.
    #[arg(long, value_name = "NAME=VALUE")]
    arg: Vec<String>,

    /// Response fields to select, comma-separated, dotted for nesting (e.g.
    /// `id,name,address.city`). Defaults to `{ __typename }` for an
    /// object-shaped return type, or no subselection for a scalar.
    #[arg(long, value_name = "FIELDS")]
    select: Option<String>,

    /// Extra request headers.
    #[arg(long, short = 'H', value_name = "KEY:VALUE")]
    header: Vec<String>,

    /// Include response headers in output.
    #[arg(long, short = 'i')]
    include: bool,

    /// Additional required OAuth scope(s), merged with the operation's.
    #[arg(long, short = 's', value_name = "SCOPE")]
    scope: Vec<String>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<GraphqlCallArgs, _, _, _>(
        CommandSpec::from_args::<GraphqlCallArgs>("call", "Call a GraphQL operation")
            .with_long(
                "Executes a GraphQL query or mutation by id (see `api operation list --domain \
                 <domain>` or `api search`). Supply each of the operation's own arguments and \
                 the wrapper's call requirements (both shown by `api graphql get <id>`) via \
                 `--arg name=value`, repeatable — the CLI builds the actual GraphQL query text \
                 and variables for you. Use `--select` to choose which response fields to get \
                 back (comma-separated, dotted for nesting, e.g. `id,name,address.city`); \
                 defaults to `{ __typename }` for an object-shaped return, or nothing for a \
                 scalar. A mutation is short-circuited under the global `--dry-run` flag.",
            )
            .with_system("api")
            .with_tier(Tier::Mutate)
            .handles_dry_run(true)
            .auth_optional(),
        |ctx, args: GraphqlCallArgs| async move {
            let id = args.id.as_str();
            let g = resolve_graphql_operation(catalog(), id)
                .ok_or_else(|| graphql_operation_not_found_error(id))?;

            // A query is safe to actually execute under `--dry-run` (same
            // treatment as GET/HEAD in `api call`'s raw-path flow); only a
            // mutation short-circuits.
            if ctx.dry_run() && g.op.kind == "mutation" {
                return Ok(CommandResult::new(json!({
                    "command": "api:graphql:call",
                    "action": "dry-run: would execute",
                    "operation": id,
                }))
                .with_dry_run());
            }

            let built = build_graphql_call(&g, &args)?;

            let required = merge_required_scopes(args.scope.clone(), &g.parent.scopes);
            let token = ctx.credential_with_scopes(&required).await?.token;
            let base_url = crate::environments::resolve_catalog_base_url(
                &g.domain.name,
                &g.domain.base_url,
                &ctx.middleware.env,
            );
            let url = format!("{base_url}{}", built.path);
            let method: reqwest::Method = g.parent.method.parse().map_err(|_| {
                crate::error::GddyError::validation(format!(
                    "invalid HTTP method on catalog entry: {}",
                    g.parent.method
                ))
                .into_cli_error()
            })?;

            let client = crate::application::client::make_http_client();
            send_and_report(
                &client,
                method,
                &g.parent.method,
                &url,
                &token,
                &built.headers,
                Some(built.body),
                args.include,
                true,
                &required,
                id,
            )
            .await
        },
    )
}

/// The pure (no I/O) result of synthesizing a GraphQL request from `--arg`/
/// `--select`: the wrapper's path with path params substituted, the extra
/// headers to send (wrapper header params plus any `--header` flags), and
/// the `{query, variables}` request body.
#[derive(Debug)]
struct BuiltGraphqlCall {
    path: String,
    headers: Vec<(String, String)>,
    body: Value,
}

/// Synthesizes a GraphQL request for `g` from `args.arg`/`args.select` —
/// the pure core of `command`'s handler, split out so it can be exercised
/// directly by tests without any auth/HTTP dependency.
fn build_graphql_call(
    g: &GraphqlOpRef<'_>,
    args: &GraphqlCallArgs,
) -> Result<BuiltGraphqlCall, cli_engine::CliCoreError> {
    let synthetic_id = graphql_operation_id(&g.parent.operation_id, &g.op.kind, &g.op.name);

    // Partition each `--arg NAME=VALUE`: a name matching one of the
    // wrapper's own parameters (e.g. `storeId`, `x-store-id`) substitutes
    // into the request; a name matching one of the GraphQL field's own args
    // becomes a query variable; anything else is a validation error listing
    // the valid names for this operation.
    let mut wrapper_values: Vec<(String, String)> = Vec::new();
    let mut variable_values: Vec<(String, String)> = Vec::new();
    for raw in &args.arg {
        let eq = raw.find('=').ok_or_else(|| {
            crate::error::GddyError::validation(format!(
                "invalid --arg '{raw}': expected name=value"
            ))
            .into_cli_error()
        })?;
        let name = &raw[..eq];
        let value = raw[eq + 1..].to_owned();
        if g.parent
            .parameters
            .iter()
            .any(|p| p.get("name").and_then(Value::as_str) == Some(name))
        {
            wrapper_values.push((name.to_owned(), value));
        } else if g.op.args.iter().any(|a| a.name == name) {
            variable_values.push((name.to_owned(), value));
        } else {
            let valid = graphql_valid_arg_names(g);
            return Err(crate::error::GddyError::validation(format!(
                "unknown --arg '{name}' for {synthetic_id} — valid names: {}",
                valid.join(", "),
            ))
            .into_cli_error());
        }
    }

    // Validate every required wrapper parameter and required GraphQL arg
    // got a value, batched into one error naming everything missing.
    let mut missing: Vec<String> = Vec::new();
    for p in &g.parent.parameters {
        let Some(name) = p.get("name").and_then(Value::as_str) else {
            continue;
        };
        let required = p.get("required").and_then(Value::as_bool).unwrap_or(false);
        if required && !wrapper_values.iter().any(|(n, _)| n == name) {
            missing.push(name.to_owned());
        }
    }
    for a in &g.op.args {
        if a.required && !variable_values.iter().any(|(n, _)| n == &a.name) {
            missing.push(a.name.clone());
        }
    }
    if !missing.is_empty() {
        return Err(crate::error::GddyError::validation(format!(
            "missing required --arg value(s) for {synthetic_id}: {}",
            missing.join(", "),
        ))
        .with_fix(format!("Run: gddy api graphql get {synthetic_id}"))
        .into_cli_error());
    }

    // Substitute wrapper path parameters into the parent's path template;
    // wrapper header parameters become request headers alongside any
    // user-supplied `--header` flags.
    let mut path = g.parent.path.clone();
    let mut extra_headers = parsed_extra_headers(&args.header)?;
    for (name, value) in &wrapper_values {
        let location = g
            .parent
            .parameters
            .iter()
            .find(|p| p.get("name").and_then(Value::as_str) == Some(name.as_str()))
            .and_then(|p| p.get("in").and_then(Value::as_str))
            .unwrap_or("query");
        match location {
            "path" => path = path.replace(&format!("{{{name}}}"), value),
            "header" => extra_headers.push((name.clone(), value.clone())),
            _ => {}
        }
    }

    // Coerce each GraphQL variable's raw CLI string per its declared type.
    let mut variables = Map::new();
    for (name, value) in &variable_values {
        if let Some(a) = g.op.args.iter().find(|a| &a.name == name) {
            variables.insert(name.clone(), coerce_graphql_arg(name, value, &a.arg_type)?);
        }
    }

    let var_decls: Vec<String> =
        g.op.args
            .iter()
            .filter(|a| variables.contains_key(&a.name))
            .map(|a| format!("${}: {}", a.name, a.arg_type))
            .collect();
    let call_args: Vec<String> =
        g.op.args
            .iter()
            .filter(|a| variables.contains_key(&a.name))
            .map(|a| format!("{0}: ${0}", a.name))
            .collect();
    let selection = build_selection(args.select.as_deref(), &g.op.return_type);

    let var_clause = if var_decls.is_empty() {
        String::new()
    } else {
        format!("({})", var_decls.join(", "))
    };
    let args_clause = if call_args.is_empty() {
        String::new()
    } else {
        format!("({})", call_args.join(", "))
    };
    let selection_clause = if selection.is_empty() {
        String::new()
    } else {
        format!(" {{ {selection} }}")
    };
    let kind = g.op.kind.as_str();
    let field = g.op.name.as_str();
    let query = format!("{kind}{var_clause} {{ {field}{args_clause}{selection_clause} }}");

    // `operationName` is deliberately omitted — optional per the wrapper's
    // own schema, and an unverified allowlist/persisted-query risk on
    // either subgraph makes always sending one unsafe to assume.
    let body = json!({ "query": query, "variables": Value::Object(variables) });

    Ok(BuiltGraphqlCall {
        path,
        headers: extra_headers,
        body,
    })
}

/// Coerces a `--arg` raw CLI string into JSON, given the GraphQL type
/// string it will be bound to as a query variable. A list type (any type
/// string containing `[`) must be a JSON array (e.g. `--arg
/// ids='["a","b"]'`) — falling back to a bare JSON string like the scalar
/// cases below would silently send the server a String where it expects a
/// List, trading a clear CLI validation error for a confusing server-side
/// type error. A non-list custom type (input object or enum) is JSON-parsed
/// first, falling back to a bare JSON string — covering `--arg
/// input='{"name":"x"}'` and `--arg status=ACTIVE` (not valid JSON on its
/// own, and a string is the right encoding either way).
fn coerce_graphql_arg(
    name: &str,
    raw: &str,
    gql_type: &str,
) -> Result<Value, cli_engine::CliCoreError> {
    if gql_type.contains('[') {
        return serde_json::from_str(raw).map_err(|e| {
            crate::error::GddyError::validation(format!(
                "invalid --arg {name}={raw}: expected a JSON array for list type {gql_type} ({e})"
            ))
            .into_cli_error()
        });
    }
    Ok(match graphql_base_type_name(gql_type) {
        "Int" => raw
            .parse::<i64>()
            .map(Value::from)
            .unwrap_or_else(|_| json!(raw)),
        "Float" => raw
            .parse::<f64>()
            .map(Value::from)
            .unwrap_or_else(|_| json!(raw)),
        "Boolean" => raw
            .parse::<bool>()
            .map(Value::from)
            .unwrap_or_else(|_| json!(raw)),
        "String" | "ID" => json!(raw),
        _ => serde_json::from_str(raw).unwrap_or_else(|_| json!(raw)),
    })
}

/// Selection-set text for a GraphQL call's `--select` flag (or its
/// default): dotted paths in `select` nest into `{ }` blocks for sibling
/// fields sharing a prefix (`"id,name,address.city"` -> `"id name address {
/// city }"`). With no `--select` at all, defaults to `__typename` when
/// `return_type`'s base name isn't one of the five built-in scalars — a
/// real custom scalar with a capitalized name would be misclassified and
/// rejected loudly by the server (not silently wrong), an acceptable
/// tradeoff against forcing `--select` on every object-returning call.
fn build_selection(select: Option<&str>, return_type: &str) -> String {
    match select.map(str::trim).filter(|s| !s.is_empty()) {
        Some(fields) => build_selection_from_fields(fields),
        None => {
            let base = graphql_base_type_name(return_type);
            if matches!(base, "Int" | "Float" | "String" | "Boolean" | "ID") {
                String::new()
            } else {
                "__typename".to_owned()
            }
        }
    }
}

/// Expands a comma-separated, dot-nested field list into GraphQL selection
/// syntax, recursively grouping siblings that share a dotted prefix.
fn build_selection_from_fields(fields: &str) -> String {
    let mut groups: Vec<(&str, Vec<&str>)> = Vec::new();
    for raw in fields.split(',') {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let (head, rest) = raw
            .split_once('.')
            .map_or((raw, None), |(h, r)| (h, Some(r)));
        match groups.iter_mut().find(|(h, _)| *h == head) {
            Some((_, subs)) => subs.extend(rest),
            None => groups.push((head, rest.into_iter().collect())),
        }
    }
    groups
        .into_iter()
        .map(|(head, subs)| {
            if subs.is_empty() {
                head.to_owned()
            } else {
                format!(
                    "{head} {{ {} }}",
                    build_selection_from_fields(&subs.join(","))
                )
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig};

    use super::super::super::catalog::{catalog, find_endpoint, resolve_graphql_operation};
    use super::{GraphqlCallArgs, build_graphql_call, build_selection, coerce_graphql_arg};

    fn graphql_cli() -> Cli {
        Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_module(crate::api_explorer::module()),
        )
    }

    fn graphql_call_args_with(id: &str, arg: Vec<&str>, select: Option<&str>) -> GraphqlCallArgs {
        GraphqlCallArgs {
            id: id.to_owned(),
            arg: arg.into_iter().map(str::to_owned).collect(),
            select: select.map(str::to_owned),
            header: vec![],
            include: false,
            scope: vec![],
        }
    }

    fn a_mutation_id() -> String {
        let (_, ep) = find_endpoint(catalog(), "postTaxGraphql")
            .expect("postTaxGraphql exists in the embedded taxes catalog");
        let schema = ep
            .graphql
            .as_ref()
            .expect("postTaxGraphql has a graphql schema");
        let op = schema
            .operations
            .iter()
            .find(|o| o.kind == "mutation")
            .expect("taxes graphql schema has at least one mutation");
        super::super::super::catalog::graphql_operation_id(&ep.operation_id, &op.kind, &op.name)
    }

    fn a_query_id() -> String {
        let (_, ep) = find_endpoint(catalog(), "postTaxGraphql")
            .expect("postTaxGraphql exists in the embedded taxes catalog");
        let schema = ep
            .graphql
            .as_ref()
            .expect("postTaxGraphql has a graphql schema");
        let op = schema
            .operations
            .iter()
            .find(|o| o.kind == "query")
            .expect("taxes graphql schema has at least one query");
        super::super::super::catalog::graphql_operation_id(&ep.operation_id, &op.kind, &op.name)
    }

    #[test]
    fn coerce_graphql_arg_scalar_and_fallback_cases() {
        assert_eq!(
            coerce_graphql_arg("n", "5", "Int").expect("valid"),
            serde_json::json!(5)
        );
        assert_eq!(
            coerce_graphql_arg("n", "5.5", "Float").expect("valid"),
            serde_json::json!(5.5)
        );
        assert_eq!(
            coerce_graphql_arg("n", "true", "Boolean").expect("valid"),
            serde_json::json!(true)
        );
        assert_eq!(
            coerce_graphql_arg("n", "abc", "String!").expect("valid"),
            serde_json::json!("abc")
        );
        assert_eq!(
            coerce_graphql_arg("n", "abc", "ID").expect("valid"),
            serde_json::json!("abc")
        );
        assert_eq!(
            coerce_graphql_arg("n", r#"["a","b"]"#, "[String!]!").expect("valid"),
            serde_json::json!(["a", "b"])
        );
        assert_eq!(
            coerce_graphql_arg("n", "ACTIVE", "StatusEnum").expect("valid"),
            serde_json::json!("ACTIVE")
        );
        assert_eq!(
            coerce_graphql_arg("n", r#"{"a":1}"#, "SomeInput").expect("valid"),
            serde_json::json!({"a": 1})
        );
    }

    #[test]
    fn coerce_graphql_arg_rejects_a_non_json_list_value() {
        let err = coerce_graphql_arg("ids", "not-json", "[String!]!")
            .expect_err("a non-JSON-array value for a list type must fail");
        let msg = err.to_string();
        assert!(msg.contains("ids"));
        assert!(msg.contains("[String!]!"));
    }

    #[test]
    fn build_selection_expands_dotted_paths_and_defaults_by_return_type_scalar_ness() {
        assert_eq!(
            build_selection(Some("id,name,address.city"), "[Widget!]!"),
            "id name address { city }"
        );
        assert_eq!(build_selection(None, "String!"), "");
        assert_eq!(build_selection(None, "[Widget!]!"), "__typename");
    }

    #[test]
    fn build_graphql_call_synthesizes_exact_query_and_variables() {
        let id = a_query_id();
        let g = resolve_graphql_operation(catalog(), &id).expect("id resolves");
        let mut args = graphql_call_args_with(&id, vec![], Some("__typename"));
        // Fill in every required wrapper param + required op arg with a
        // placeholder so this exercises synthesis, not validation.
        for p in &g.parent.parameters {
            if let Some(name) = p.get("name").and_then(serde_json::Value::as_str) {
                args.arg.push(format!("{name}=placeholder"));
            }
        }
        for a in &g.op.args {
            args.arg.push(format!("{}=placeholder", a.name));
        }
        let built = build_graphql_call(&g, &args).expect("builds without error");
        assert!(
            built.body["query"]
                .as_str()
                .expect("query is a string")
                .contains(&g.op.name)
        );
        assert!(
            !built.path.contains('{'),
            "path params substituted: {}",
            built.path
        );
    }

    #[tokio::test]
    async fn graphql_call_dry_run_short_circuits_mutation_but_not_query() {
        let mutation_id = a_mutation_id();
        let output = graphql_cli()
            .run([
                "gddy",
                "api",
                "graphql",
                "call",
                &mutation_id,
                "--dry-run",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        assert!(output.rendered.contains("dry-run: would execute"));

        // A query has nothing unsafe to preview, so it isn't short-circuited
        // — it falls through to a real (auth-requiring, here failing) send.
        let query_id = a_query_id();
        let output = graphql_cli()
            .run([
                "gddy",
                "api",
                "graphql",
                "call",
                &query_id,
                "--dry-run",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            !output.rendered.contains("dry-run: would execute"),
            "{}",
            output.rendered
        );
    }

    #[test]
    fn build_graphql_call_missing_required_arg_is_validation_error() {
        let id = a_query_id();
        let g = resolve_graphql_operation(catalog(), &id).expect("id resolves");
        let args = graphql_call_args_with(&id, vec![], None);
        let err = build_graphql_call(&g, &args).expect_err("missing required args");
        assert!(err.to_string().contains("missing required"), "{err}");
    }

    #[test]
    fn build_graphql_call_unknown_arg_name_is_validation_error() {
        let id = a_query_id();
        let g = resolve_graphql_operation(catalog(), &id).expect("id resolves");
        let args = graphql_call_args_with(&id, vec!["notARealArgName=x"], None);
        let err = build_graphql_call(&g, &args).expect_err("unknown arg name");
        assert!(err.to_string().contains("unknown --arg"), "{err}");
    }
}
