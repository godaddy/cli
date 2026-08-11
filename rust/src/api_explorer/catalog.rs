//! The embedded API catalog: data model, static parsing, and lookup.
//!
//! Every domain's OpenAPI-derived spec is embedded at compile time (see
//! [`DOMAIN_FILES`]) and parsed once into [`catalog`]'s `'static` slice.
//! Lookup helpers here range from exact (operationId/path/method) to fuzzy
//! (substring across path/summary/description/GraphQL operation names), used
//! by every command in this module — `api domain list`, `api operation
//! list/get`, `api parameter`/`api response` (scoped by `--operation`), `api
//! search`, and `api call`.

use std::sync::OnceLock;

use cli_engine::CliCoreError;
use serde::Deserialize;
use serde_json::{Map, Value};

// ---------------------------------------------------------------------------
// Catalog types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(super) struct Domain {
    pub(super) name: String,
    pub(super) title: String,
    pub(super) description: String,
    #[serde(rename = "baseUrl")]
    pub(super) base_url: String,
    pub(super) endpoints: Vec<Endpoint>,
    /// Local JSON-schema definitions (`$defs`) that a `requestBody`/response
    /// schema's `$ref` may point at, e.g. `{"$ref": "#/$defs/Business"}`.
    /// Most catalog request bodies are a bare `$ref` with no inline
    /// `properties`, so resolving these is required for schema
    /// summarization to say anything useful about them.
    #[serde(rename = "$defs", default)]
    pub(super) defs: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Endpoint {
    #[serde(rename = "operationId")]
    pub(super) operation_id: String,
    pub(super) method: String,
    pub(super) path: String,
    pub(super) summary: String,
    #[serde(default)]
    pub(super) description: String,
    #[serde(default)]
    pub(super) parameters: Vec<Value>,
    #[serde(rename = "requestBody", default)]
    pub(super) request_body: Option<Value>,
    #[serde(default)]
    pub(super) responses: Value,
    #[serde(default)]
    pub(super) scopes: Vec<String>,
    #[serde(default)]
    pub(super) graphql: Option<GraphqlSchema>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GraphqlArgument {
    pub(super) name: String,
    #[serde(rename = "type")]
    pub(super) arg_type: String,
    pub(super) required: bool,
    #[serde(default)]
    pub(super) description: Option<String>,
    #[serde(default, rename = "defaultValue")]
    pub(super) default_value: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GraphqlOperation {
    pub(super) name: String,
    pub(super) kind: String,
    #[serde(rename = "returnType")]
    pub(super) return_type: String,
    #[serde(default)]
    pub(super) description: Option<String>,
    #[serde(default)]
    pub(super) deprecated: bool,
    #[serde(default, rename = "deprecationReason")]
    pub(super) deprecation_reason: Option<String>,
    #[serde(default)]
    pub(super) args: Vec<GraphqlArgument>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GraphqlField {
    pub(super) name: String,
    #[serde(rename = "type")]
    pub(super) field_type: String,
    #[serde(default)]
    pub(super) description: Option<String>,
}

/// A named GraphQL object/input/enum type — used by `api graphql get`
/// (resolving an operation's return type into its real field list) and `api
/// graphql type get` (looking one up directly by name).
#[derive(Debug, Deserialize)]
pub(super) struct GraphqlType {
    pub(super) name: String,
    pub(super) kind: String,
    #[serde(default)]
    pub(super) fields: Vec<GraphqlField>,
    #[serde(default)]
    pub(super) values: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GraphqlSchema {
    #[serde(rename = "schemaRef")]
    pub(super) schema_ref: String,
    #[serde(rename = "operationCount")]
    pub(super) operation_count: usize,
    pub(super) operations: Vec<GraphqlOperation>,
    #[serde(default)]
    pub(super) types: Vec<GraphqlType>,
    /// The raw GraphQL SDL source verbatim — see `api graphql sdl get`.
    #[serde(default)]
    pub(super) sdl: String,
}

// ---------------------------------------------------------------------------
// Static catalog — parsed once from embedded JSON
// ---------------------------------------------------------------------------

static CATALOG: OnceLock<Vec<Domain>> = OnceLock::new();

const DOMAIN_FILES: &[(&str, &str)] = &[
    (
        "bulk-operations",
        include_str!("../../schemas/api/bulk-operations.json"),
    ),
    (
        "businesses",
        include_str!("../../schemas/api/businesses.json"),
    ),
    (
        "catalog-products",
        include_str!("../../schemas/api/catalog-products.json"),
    ),
    ("channels", include_str!("../../schemas/api/channels.json")),
    (
        "chargebacks",
        include_str!("../../schemas/api/chargebacks.json"),
    ),
    (
        "customer-profiles",
        include_str!("../../schemas/api/customer-profiles.json"),
    ),
    (
        "fulfillments",
        include_str!("../../schemas/api/fulfillments.json"),
    ),
    (
        "location-addresses",
        include_str!("../../schemas/api/location-addresses.json"),
    ),
    (
        "metafields",
        include_str!("../../schemas/api/metafields.json"),
    ),
    (
        "onboarding",
        include_str!("../../schemas/api/onboarding.json"),
    ),
    ("orders", include_str!("../../schemas/api/orders.json")),
    (
        "payment-requests",
        include_str!("../../schemas/api/payment-requests.json"),
    ),
    ("payments", include_str!("../../schemas/api/payments.json")),
    (
        "price-adjustments",
        include_str!("../../schemas/api/price-adjustments.json"),
    ),
    (
        "recommendations",
        include_str!("../../schemas/api/recommendations.json"),
    ),
    ("shipping", include_str!("../../schemas/api/shipping.json")),
    ("stores", include_str!("../../schemas/api/stores.json")),
    (
        "subscriptions",
        include_str!("../../schemas/api/subscriptions.json"),
    ),
    ("taxes", include_str!("../../schemas/api/taxes.json")),
    (
        "transactions",
        include_str!("../../schemas/api/transactions.json"),
    ),
    (
        "hosting-nodejs",
        include_str!("../../schemas/api/hosting-nodejs.json"),
    ),
    ("domains", include_str!("../../schemas/api/domains.json")),
];

pub(super) fn catalog() -> &'static [Domain] {
    CATALOG.get_or_init(|| {
        let mut domains: Vec<Domain> = DOMAIN_FILES
            .iter()
            .filter_map(|(_, src)| serde_json::from_str::<Domain>(src).ok())
            .collect();
        // Sorted once here (not per-listing) so every consumer — `api domain
        // list`, `api search`, `api operation get` — sees the same stable order.
        domains.sort_by(|a, b| a.name.cmp(&b.name));
        domains
    })
}

/// True if `concrete`'s path segments structurally match `template`'s: a
/// `{param}` segment in `template` matches any single non-empty segment in
/// `concrete` at the same position, every other segment must match
/// literally (case-insensitive). Matches a real request path with path
/// params substituted (e.g. `/stores/abc123/orders`) against the catalog's
/// templated path (`/stores/{storeId}/orders`), which neither exact nor
/// substring matching can do — a concrete path never literally contains the
/// `{storeId}` placeholder.
fn path_matches_template(template: &str, concrete: &str) -> bool {
    let template_segs: Vec<&str> = template.trim_matches('/').split('/').collect();
    let concrete_segs: Vec<&str> = concrete.trim_matches('/').split('/').collect();
    template_segs.len() == concrete_segs.len()
        && template_segs
            .iter()
            .zip(concrete_segs.iter())
            .all(|(t, c)| {
                (t.starts_with('{') && t.ends_with('}') && !c.is_empty())
                    || t.eq_ignore_ascii_case(c)
            })
}

pub(super) fn find_endpoint<'a>(
    catalog: &'a [Domain],
    query: &str,
) -> Option<(&'a Domain, &'a Endpoint)> {
    let q = query.to_lowercase();
    catalog.iter().find_map(|domain| {
        domain.endpoints.iter().find_map(|ep| {
            if ep.operation_id.to_lowercase() == q
                || ep.path.to_lowercase() == q
                || path_matches_template(&ep.path, query)
                || ep.path.to_lowercase().contains(&q)
            {
                Some((domain, ep))
            } else {
                None
            }
        })
    })
}

/// Exact (non-fuzzy) endpoint lookup by operationId, full path equality, or
/// a concrete path against a templated catalog path (e.g. `/stores/abc123`
/// against `/stores/{storeId}`), optionally narrowed to a specific HTTP
/// method. Used by `api operation get`'s primary resolution step, distinct from
/// `find_endpoint`'s looser substring-`contains` match (which `api call`
/// still relies on for scope resolution and is left untouched).
///
/// Returns every match, not just the first: many catalog paths are shared by
/// several endpoints that only differ by method (e.g. `GET`/`POST` on the
/// same collection endpoint), so without `--method` there can genuinely be
/// more than one exact match — the caller decides what to do with that
/// (typically: 1 match resolves transparently, >1 is treated the same as an
/// ambiguous fuzzy match).
fn find_endpoint_exact<'a>(
    catalog: &'a [Domain],
    query: &str,
    method: Option<&str>,
) -> Vec<(&'a Domain, &'a Endpoint)> {
    let q = query.to_lowercase();
    catalog
        .iter()
        .flat_map(|domain| {
            let q = q.clone();
            domain.endpoints.iter().filter_map(move |ep| {
                let path_matches = ep.operation_id.to_lowercase() == q
                    || ep.path.to_lowercase() == q
                    || path_matches_template(&ep.path, query);
                let method_matches = method.is_none_or(|m| ep.method.eq_ignore_ascii_case(m));
                (path_matches && method_matches).then_some((domain, ep))
            })
        })
        .collect()
}

/// Builds an "ambiguous match" error for `resolve_operation`'s two >1-hit
/// branches — an exact path shared by several HTTP methods, or a fuzzy
/// query hitting several unrelated endpoints. Every candidate is formatted
/// as a runnable `gddy api operation get <path> --method <method>` line and
/// joined into the error's `fix` field: cli-engine's error envelope has no
/// hook today for a `DetailedError` to attach structured `next_actions`
/// (`build_error_envelope` always sets `next_actions: Vec::new()`), so a
/// formatted `fix` string is the richest thing available until that's
/// closed upstream (filed as a follow-up ticket, matching DEVEX-968/972/981
/// this session).
fn ambiguous_operation_error(query: &str, hits: &[(&Domain, &Endpoint)]) -> CliCoreError {
    let candidates = hits
        .iter()
        .map(|(_, ep)| {
            format!(
                "gddy api operation get {} --method {}  # {}",
                ep.path, ep.method, ep.summary
            )
        })
        .collect::<Vec<_>>()
        .join("\n  ");
    crate::error::GddyError::ambiguous(format!(
        "'{query}' matches {} operations. Be more specific:",
        hits.len()
    ))
    .with_fix(format!("Run one of:\n  {candidates}"))
    .into_cli_error()
}

/// Resolves a `--operation`/positional operation query against the catalog
/// — shared by `operation get` and every command scoped by `--operation`
/// (`parameter`/`response` list and get, `schema get`), since they all need
/// the same exact/fuzzy/ambiguous cascade `operation_get_command` already
/// implemented before this existed. Both "no match" and "more than one
/// match" are errors — there's no single operation to act on either way.
pub(super) fn resolve_operation<'a>(
    catalog: &'a [Domain],
    query: &str,
    method_filter: Option<&str>,
) -> Result<(&'a Domain, &'a Endpoint), CliCoreError> {
    let exact_hits = find_endpoint_exact(catalog, query, method_filter);
    match exact_hits.len() {
        1 => Ok((exact_hits[0].0, exact_hits[0].1)),
        0 => {
            let hits = search_endpoints(catalog, query);
            match hits.len() {
                0 => Err(crate::error::GddyError::not_found(format!(
                    "no operation found matching '{query}' — try `gddy api search {query}`"
                ))
                .with_fix(format!(
                    "Run: gddy api search {query} or gddy api domain list"
                ))
                .into_cli_error()),
                1 => Ok((hits[0].0, hits[0].1)),
                _ => Err(ambiguous_operation_error(query, &hits)),
            }
        }
        _ => Err(ambiguous_operation_error(query, &exact_hits)),
    }
}

pub(super) fn search_endpoints<'a>(
    catalog: &'a [Domain],
    query: &str,
) -> Vec<(&'a Domain, &'a Endpoint)> {
    let q = query.to_lowercase();
    catalog
        .iter()
        .flat_map(|domain| {
            let q = q.clone();
            domain.endpoints.iter().filter_map(move |ep| {
                let haystack = format!(
                    "{} {} {} {}",
                    ep.operation_id.to_lowercase(),
                    ep.path.to_lowercase(),
                    ep.summary.to_lowercase(),
                    ep.description.to_lowercase(),
                );
                // Only scan GraphQL operations (up to 149 on some domains)
                // when the core fields didn't already match, and stop at the
                // first hit instead of concatenating every operation into one
                // string up front.
                let matches = haystack.contains(&q)
                    || ep.graphql.as_ref().is_some_and(|g| {
                        g.operations.iter().any(|op| {
                            format!("{} {}", op.kind, op.name)
                                .to_lowercase()
                                .contains(&q)
                        })
                    });
                if matches { Some((domain, ep)) } else { None }
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// GraphQL operation ids and resolution
// ---------------------------------------------------------------------------
//
// A GraphQL query/mutation field isn't its own catalog endpoint — it's
// addressed as `<parentOperationId>::<query|mutation>::<fieldName>`, e.g.
// `postTaxGraphql::query::classification`, resolved against the wrapper
// endpoint's own `graphql.operations`. `api graphql get`/`api graphql call`
// are this id's only real targets; `operation get`/`parameter list`/`api
// call` redirect to them instead of rendering REST-shaped output for a
// GraphQL operation (see `graphql_operation_redirect_error`).

pub(super) fn graphql_operation_id(parent_operation_id: &str, kind: &str, name: &str) -> String {
    format!("{parent_operation_id}::{kind}::{name}")
}

fn parse_graphql_op_id(id: &str) -> Option<(&str, &str, &str)> {
    let mut parts = id.splitn(3, "::");
    let parent = parts.next()?;
    let kind = parts.next()?;
    let name = parts.next()?;
    (!parent.is_empty() && !name.is_empty() && matches!(kind, "query" | "mutation"))
        .then_some((parent, kind, name))
}

pub(super) struct GraphqlOpRef<'a> {
    pub(super) domain: &'a Domain,
    pub(super) parent: &'a Endpoint,
    pub(super) op: &'a GraphqlOperation,
}

pub(super) fn resolve_graphql_operation<'a>(
    catalog: &'a [Domain],
    id: &str,
) -> Option<GraphqlOpRef<'a>> {
    let (parent_id, kind, name) = parse_graphql_op_id(id)?;
    let (domain, parent) = find_endpoint_exact(catalog, parent_id, None)
        .into_iter()
        .next()?;
    let op = parent
        .graphql
        .as_ref()?
        .operations
        .iter()
        .find(|o| o.kind == kind && o.name == name)?;
    Some(GraphqlOpRef { domain, parent, op })
}

/// Error for `operation get`/`parameter list`/`parameter get`/`call` given a
/// GraphQL operation id — these commands render REST-shaped output that
/// doesn't fit a GraphQL operation at all, so they redirect to the dedicated
/// `api graphql` equivalent instead of trying to render anything.
pub(super) fn graphql_operation_redirect_error(id: &str, next: &str) -> CliCoreError {
    crate::error::GddyError::validation(format!(
        "'{id}' is a GraphQL operation — use `gddy api {next} {id}` instead"
    ))
    .with_fix(format!("Run: gddy api {next} {id}"))
    .into_cli_error()
}

/// Error for any `api graphql` command given an id that doesn't resolve to a
/// real GraphQL operation — either malformed, or naming a domain/wrapper
/// operation that exists but isn't itself addressable this way (e.g. the
/// wrapper id `postTaxGraphql` rather than one of its operations).
pub(super) fn graphql_operation_not_found_error(id: &str) -> CliCoreError {
    crate::error::GddyError::not_found(format!(
        "no GraphQL operation found for id '{id}' — expected \
         <parentOperationId>::<query|mutation>::<fieldName>, e.g. \
         postTaxGraphql::query::classification"
    ))
    .with_fix("Run: gddy api operation list --domain <domain>, or gddy api search <query>")
    .into_cli_error()
}

/// Every GraphQL query/mutation field on `ep`'s schema, as a synthetic
/// endpoint-list row addressed via [`graphql_operation_id`] — appended
/// alongside the wrapper's own row by `operation_list_command`/
/// `search_command` so a GraphQL operation is just as discoverable as a
/// REST one.
pub(super) fn graphql_sub_endpoint_rows(domain_name: Option<&str>, ep: &Endpoint) -> Vec<Value> {
    let Some(schema) = ep.graphql.as_ref() else {
        return Vec::new();
    };
    schema
        .operations
        .iter()
        .map(|op| {
            let mut row = serde_json::json!({
                "operationId": graphql_operation_id(&ep.operation_id, &op.kind, &op.name),
                "method": ep.method,
                "path": ep.path,
                "summary": op.description.clone().unwrap_or_else(|| format!("{} {}", op.kind, op.name)),
                "scopes": ep.scopes,
                "kind": op.kind,
            });
            if let Some(name) = domain_name {
                row["domain"] = serde_json::json!(name);
            }
            row
        })
        .collect()
}

/// Strips GraphQL's Non-Null (`!`) and List (`[...]`) type modifiers down to
/// the bare named type, e.g. `"[Widget!]!"` -> `"Widget"` — the name that
/// `GraphqlType::name` and `coerce_graphql_arg`'s scalar match key off of.
pub(super) fn graphql_base_type_name(type_str: &str) -> &str {
    type_str.trim_matches(|c: char| matches!(c, '!' | '[' | ']'))
}

/// Looks up a named GraphQL type (object/input/enum) by its bare name
/// against the domain schema `g` belongs to — used to resolve an operation's
/// `returnType` string into its real field list. `type_str` may be a full
/// GraphQL type string (e.g. `"[Widget!]!"`); only its base name is looked
/// up (see [`graphql_base_type_name`]).
pub(super) fn graphql_resolve_type<'a>(
    g: &GraphqlOpRef<'a>,
    type_str: &str,
) -> Option<&'a GraphqlType> {
    let base_name = graphql_base_type_name(type_str);
    g.parent
        .graphql
        .as_ref()?
        .types
        .iter()
        .find(|t| t.name == base_name)
}

/// Every domain whose own GraphQL schema defines a type named `name` — the
/// standalone counterpart to [`graphql_resolve_type`] for `api graphql type
/// get`, which has no operation to scope the search to. A GraphQL type name
/// is only unique *within* a single subgraph; two independently-authored
/// domain schemas can (and do — e.g. `ReferenceValueFilter` on both `taxes`
/// and `catalog-products`, with different fields) declare their own,
/// unrelated type sharing a name. Callers must handle more than one hit
/// rather than assume the first is the one the user meant.
pub(super) fn find_graphql_types<'a>(
    catalog: &'a [Domain],
    name: &str,
) -> Vec<(&'a Domain, &'a GraphqlType)> {
    catalog
        .iter()
        .filter_map(|d| {
            d.endpoints
                .iter()
                .find_map(|ep| ep.graphql.as_ref()?.types.iter().find(|t| t.name == name))
                .map(|t| (d, t))
        })
        .collect()
}

/// Error for `api graphql type get` given a name that doesn't resolve to any
/// known GraphQL object/input/enum (in the requested domain, if `--domain`
/// was given).
pub(super) fn graphql_type_not_found_error(name: &str) -> CliCoreError {
    crate::error::GddyError::not_found(format!("no GraphQL type found named '{name}'"))
        .with_fix(
            "Run: gddy api graphql get <operationId> and check a field's Type column for the \
             exact name to look up",
        )
        .into_cli_error()
}

/// Error for `api graphql type get` when `name` resolves in more than one
/// domain's GraphQL schema with no `--domain` given to disambiguate.
pub(super) fn ambiguous_graphql_type_error(name: &str, domains: &[&str]) -> CliCoreError {
    crate::error::GddyError::ambiguous(format!(
        "'{name}' is defined by {} different domains ({}) with no guarantee they're the same \
         shape — be more specific:",
        domains.len(),
        domains.join(", "),
    ))
    .with_fix(format!(
        "Run: gddy api graphql type get {name} --domain <domain>, one of: {}",
        domains.join(", "),
    ))
    .into_cli_error()
}

/// Every valid `--arg` name for `g`'s call — the wrapper's own parameters
/// plus the GraphQL field's own arguments — used in the "unknown --arg"
/// validation error's suggestion list.
pub(super) fn graphql_valid_arg_names(g: &GraphqlOpRef<'_>) -> Vec<String> {
    g.parent
        .parameters
        .iter()
        .filter_map(|p| p.get("name").and_then(Value::as_str).map(str::to_owned))
        .chain(g.op.args.iter().map(|a| a.name.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{catalog, find_endpoint};

    /// `catalog()` sorts once so every listing (`api domain list`, `api
    /// search`, `api operation get`) sees the same stable, alphabetical order.
    #[test]
    fn catalog_domains_are_sorted_alphabetically() {
        let names: Vec<&str> = catalog().iter().map(|d| d.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    /// Mirrors `call_command`'s domain-aware base-URL resolution: a matched
    /// catalog endpoint resolves through its own domain's host (not the
    /// generic gateway), with per-environment convention substitution.
    #[test]
    fn matched_endpoint_resolves_its_own_domain_base_url() {
        let (domain, _) = find_endpoint(catalog(), "listFulfillments")
            .expect("listFulfillments exists in the embedded catalog");
        assert_eq!(domain.name, "fulfillments");
        let base_url =
            crate::environments::resolve_catalog_base_url(&domain.name, &domain.base_url, "ote");
        assert_eq!(
            base_url,
            "https://fulfillment.api.commerce.ote-godaddy.com/v1/commerce"
        );
    }

    /// A real request path with path params substituted (as `api call`
    /// receives — no user passes a literal `{storeId}`) must still match its
    /// templated catalog path, or every parameterized endpoint would fall
    /// back to the wrong (generic gateway) base URL and lose scope lookup.
    #[test]
    fn concrete_path_matches_its_templated_catalog_path() {
        let (domain, endpoint) = find_endpoint(catalog(), "/stores/abc123/fulfillments")
            .expect("concrete path should match the templated catalog path");
        assert_eq!(domain.name, "fulfillments");
        assert_eq!(endpoint.operation_id, "listFulfillments");
    }

    /// An endpoint the catalog doesn't recognize has no domain to resolve a
    /// base URL against — `call_command` falls back to the generic gateway
    /// host in this case.
    #[test]
    fn unmatched_endpoint_has_no_domain_to_resolve_against() {
        assert!(find_endpoint(catalog(), "not-a-real-operation-id").is_none());
    }

    // -----------------------------------------------------------------------
    // search_endpoints — GraphQL operation names are searchable
    // -----------------------------------------------------------------------

    #[test]
    fn search_endpoints_matches_a_graphql_operation_name() {
        let hits = super::search_endpoints(catalog(), "SKUGroup");
        assert!(
            hits.iter()
                .any(|(_, ep)| ep.operation_id == "postCatalogGraphql"),
            "expected a GraphQL operation name to surface its parent endpoint in search results"
        );
    }
}
