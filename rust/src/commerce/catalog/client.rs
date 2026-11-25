//! The client every `gddy commerce catalog` command uses to run a typed GraphQL
//! operation against a store's catalog subgraph.
//!
//! It adds what the subgraph needs on top of `graphql_client`: the store-scoped
//! path, the `x-store-id` header, unset variables left out of the request, and
//! failures mapped to coded `commerce` errors. Requests go through the shared
//! HTTP client builder and the transport observer, like the other clients, but
//! the observer is shown only methods, URLs, headers and statuses: the catalog
//! holds merchant data, so request and response bodies stay out of
//! `--debug transport` traces.

use std::collections::BTreeSet;

use cli_engine::{CliCoreError, CommandContext, Result};
use generated_client_support::{
    build_authenticated_http_client, execute_and_observe_metadata_only,
    notify_request_metadata_only, validate_base_url,
};
use graphql_client::{Error as GraphqlError, GraphQLQuery, Response};
use reqwest::{Client as HttpClient, ClientBuilder};
use serde_json::{Value, from_value};
use uuid::Uuid;

use super::operations::{ProductList, product_list};
use crate::{
    api::http::encode_path_segment, commerce::client::http_error, environments, error::GddyError,
    http,
};

/// Name the catalog is reported under in error envelopes.
const SYSTEM: &str = "commerce";

/// The `extensions.code` values this client acts on. Everything else an error
/// carries is the API's own free text, which can quote merchant values, so it is
/// never shown: messages are written here, keyed by code.
///
/// A request the subgraph refuses for being malformed.
const VALIDATION_ERROR: &str = "VALIDATION_ERROR";
/// A caller the subgraph will not serve for this store.
const UNAUTHORIZED: &str = "UNAUTHORIZED";
/// A query the subgraph's schema does not accept, which is a bug in this client.
const QUERY_REJECTED: &str = "GRAPHQL_VALIDATION_FAILED";

/// What to try for a failure that no change to the command's input can repair:
/// a query gddy itself sends, or an answer it cannot read. The default hint for
/// an unexpected error says to retry with corrected input, which would send the
/// user looking for a mistake they did not make.
const NOT_FIXABLE_BY_INPUT: &str = "Retry. If it keeps failing, update gddy with `gddy update apply` and \
     report it, naming the message above.";

/// A client for one store's catalog subgraph.
pub(super) struct CatalogClient {
    http: HttpClient,
    base_url: String,
    store_id: String,
    env: String,
}

impl CatalogClient {
    /// Builds a client that sends `token` as a bearer credential.
    ///
    /// `builder` carries the shared transport policy (user agent, timeouts).
    /// `store_id` must already be a canonical store id; `env` only shapes the
    /// text of errors about a store that is not there.
    ///
    /// # Errors
    ///
    /// A config error if `base_url` is not HTTPS (plain HTTP is allowed only on
    /// a loopback host), or if the credential or client cannot be built.
    pub(super) fn new(
        builder: ClientBuilder,
        base_url: &str,
        token: &str,
        store_id: &str,
        env: &str,
    ) -> Result<Self> {
        validate_base_url(base_url).map_err(config_error)?;
        let http = build_authenticated_http_client(
            builder,
            &format!("Bearer {token}"),
            &Uuid::new_v4().to_string(),
        )
        .map_err(config_error)?;
        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
            store_id: store_id.to_owned(),
            env: env.to_owned(),
        })
    }

    /// A client for the active environment, authenticated with a token that
    /// carries `scopes` (prompting for consent if the login lacks them).
    pub(super) async fn connect(
        ctx: &CommandContext,
        store_id: &str,
        scopes: &[&str],
    ) -> Result<Self> {
        http::ensure_generated_client_transport_observer_registered();
        let required: Vec<String> = scopes.iter().map(|scope| (*scope).to_owned()).collect();
        let token = ctx.credential_with_scopes(&required).await?.token;
        let env = &ctx.middleware.env;
        let base_url = environments::resolve(env)?.api_url;
        Self::new(http::api_client_builder(), &base_url, &token, store_id, env)
    }

    /// Runs one operation and returns its typed data.
    ///
    /// A non-empty `errors` array is a failure even on HTTP 200 and even beside
    /// partial `data`: a caller wants a complete result or the reason it is
    /// missing. No part of a response body reaches an error, and neither does the
    /// text of an `errors` entry: see [`graphql_error`].
    ///
    /// `fields` are the root fields the operation selects. A server answers with
    /// an entry for each, `null` when there is nothing, so one that is absent
    /// means the answer is not an answer to this query. Without that check a
    /// `data` that is empty, or holds something else, reads as "no results", and
    /// a broken response would look like an empty catalog.
    async fn send<Q: GraphQLQuery>(
        &self,
        variables: Q::Variables,
        fields: &[&str],
    ) -> Result<Q::ResponseData> {
        let mut payload = serde_json::to_value(Q::build_query(variables)).map_err(|_| {
            GddyError::unexpected("the catalog request could not be built")
                .with_fix(NOT_FIXABLE_BY_INPUT)
                .with_system(SYSTEM)
                .into_cli_error()
        })?;
        if let Some(variables) = payload.get_mut("variables") {
            omit_unset(variables);
        }

        let url = format!(
            "{}/v2/commerce/stores/{}/catalog-subgraph",
            self.base_url,
            encode_path_segment(&self.store_id)
        );
        let request = self
            .http
            .post(url)
            .header("x-store-id", &self.store_id)
            .json(&payload)
            .build()
            .map_err(|error| {
                GddyError::unexpected(format!("the catalog request could not be built: {error}"))
                    .with_fix(NOT_FIXABLE_BY_INPUT)
                    .with_system(SYSTEM)
                    .into_cli_error()
            })?;
        notify_request_metadata_only(&request);
        let response = execute_and_observe_metadata_only(&self.http, request)
            .await
            .map_err(network_error)?;

        // An error status is mapped by the status alone, so its body is never
        // read: a body that is cut off or times out must not turn a 404 or 401
        // into a network error and lose the advice that goes with it.
        let status = response.status();
        if !status.is_success() {
            return Err(http_error(status.as_u16(), &self.store_id, &self.env));
        }
        let bytes = response.bytes().await.map_err(network_error)?;

        // serde's message can quote the value it choked on, so only the position
        // of a syntax error is reported, and nothing of a shape mismatch.
        let parsed: Response<Value> = serde_json::from_slice(&bytes).map_err(|error| {
            GddyError::unexpected(format!(
                "the Commerce API sent an answer that could not be read (line {}, column {})",
                error.line(),
                error.column()
            ))
            .with_fix(NOT_FIXABLE_BY_INPUT)
            .with_system(SYSTEM)
            .into_cli_error()
        })?;
        if let Some(errors) = parsed.errors.filter(|errors| !errors.is_empty()) {
            return Err(graphql_error(&errors, &self.store_id, &self.env));
        }
        let data = parsed
            .data
            .ok_or_else(|| malformed("it had neither data nor an error"))?;
        if !answers(&data, fields) {
            return Err(malformed("it did not contain what was asked for"));
        }
        from_value(data).map_err(|_| malformed("it was not shaped as expected"))
    }

    /// One page of products (`skuGroups`), `first` at a time after the cursor
    /// `after`, optionally only those whose status is one of `status`. `None`
    /// when the store has no products connection at all, which the schema allows
    /// and which is not the same as an empty page.
    pub(super) async fn list_products(
        &self,
        first: i64,
        after: Option<&str>,
        status: Option<&[String]>,
    ) -> Result<Option<product_list::ProductListSkuGroups>> {
        let data = self
            .send::<ProductList>(
                product_list::Variables {
                    first: Some(first),
                    after: after.map(str::to_owned),
                    status: status.map(|values| product_list::SKUGroupStatusFilter {
                        eq: None,
                        in_: Some(values.to_vec()),
                    }),
                },
                &["skuGroups"],
            )
            .await?;
        if let Some(page) = &data.sku_groups {
            check_page(page)?;
        }
        Ok(data.sku_groups)
    }
}

/// Removes every object member that is `null` from `variables`, at any depth.
///
/// `graphql_client` writes an unset optional as `null`, but the subgraph rejects
/// that: an unset filter variable is a `VALIDATION_ERROR`, and so is an unset
/// member inside a filter that is set (`{ "eq": null, "in": ["ACTIVE"] }`),
/// while the same query with the member absent succeeds. An optional that is
/// `None` is the only way a `null` object member gets here, so removing them is
/// exactly "leave out what was not set".
///
/// A `null` that is an element of a list is a value, not an unset member, and is
/// kept, as is every other value, including `false`, `0` and the empty string.
fn omit_unset(variables: &mut Value) {
    match variables {
        Value::Object(members) => {
            members.retain(|_, member| !member.is_null());
            members.values_mut().for_each(omit_unset);
        }
        Value::Array(items) => items.iter_mut().for_each(omit_unset),
        _ => {}
    }
}

/// A coded error for what the API reported in a response's `errors` array.
///
/// Only the `extensions.code` of each error decides the result. An error's own
/// `message` is the API's free text and can quote the merchant values that were
/// submitted, including in a 200 response, so it is never shown, and neither is
/// anything else in the error that is not a plain identifier.
fn graphql_error(errors: &[GraphqlError], store_id: &str, env: &str) -> CliCoreError {
    let has = |code: &str| errors.iter().any(|error| error_code(error) == Some(code));
    if has(UNAUTHORIZED) {
        return http_error(403, store_id, env);
    }
    if has(VALIDATION_ERROR) {
        return GddyError::validation(describe_refusal(errors))
            .with_system(SYSTEM)
            .into_cli_error();
    }
    if has(QUERY_REJECTED) {
        return GddyError::unexpected(
            "the Commerce API rejected a query this version of gddy sends; this is a bug in gddy",
        )
        .with_fix(NOT_FIXABLE_BY_INPUT)
        .with_system(SYSTEM)
        .into_cli_error();
    }
    let codes: BTreeSet<&str> = errors
        .iter()
        .filter_map(error_code)
        .filter(|code| is_plain(code))
        .collect();
    let message = if codes.is_empty() {
        "the Commerce API reported an error".to_owned()
    } else {
        let codes: Vec<&str> = codes.into_iter().collect();
        format!("the Commerce API reported an error ({})", codes.join(", "))
    };
    GddyError::from_graphql(message, SYSTEM)
        .with_fix("Try again. If it keeps failing, report it with the message above.")
        .into_cli_error()
}

fn error_code(error: &GraphqlError) -> Option<&str> {
    error.extensions.as_ref()?.get("code")?.as_str()
}

/// What a refused request says, built from the fields the API lists beside its
/// message. Each field's code and path must be plain identifiers; one that is
/// not is left out rather than shown.
fn describe_refusal(errors: &[GraphqlError]) -> String {
    let fields: BTreeSet<String> = errors
        .iter()
        .filter(|error| error_code(error) == Some(VALIDATION_ERROR))
        .filter_map(|error| error.extensions.as_ref()?.get("fields")?.as_array())
        .flatten()
        .filter_map(describe_field)
        .collect();
    if fields.is_empty() {
        "the request was not valid".to_owned()
    } else {
        let fields: Vec<String> = fields.into_iter().collect();
        format!("the request was not valid: {}", fields.join(", "))
    }
}

fn describe_field(field: &Value) -> Option<String> {
    let code = field.get("code")?.as_str().filter(|code| is_plain(code))?;
    let path = field
        .get("path")?
        .as_array()?
        .iter()
        .map(|part| match part {
            Value::String(name) => is_plain(name).then(|| name.clone()),
            Value::Number(index) => index.as_u64().map(|index| index.to_string()),
            _ => None,
        })
        .collect::<Option<Vec<String>>>()?;
    Some(format!("{} ({code})", path.join(".")))
}

/// Whether `text` is a short identifier (letters, digits, `_`, `-`, `.`), the
/// only kind of API-supplied text that is shown: a code or a field name, never a
/// sentence that could contain a value.
fn is_plain(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// Whether `data` has an entry for every root field in `fields`. A server
/// answers with one for each field a query selects, `null` when there is nothing,
/// so a missing one means the answer is not an answer to the query.
fn answers(data: &Value, fields: &[&str]) -> bool {
    data.as_object()
        .is_some_and(|data| fields.iter().all(|field| data.contains_key(*field)))
}

/// What is wrong with a page that is not self-consistent, as an error that does
/// not quote the page. A caller that paged on `hasNextPage` would loop or stop
/// early on one with no cursor, and a null edge or node would drop a product
/// from the list without a word.
fn check_page(page: &product_list::ProductListSkuGroups) -> Result<()> {
    let has_cursor = page
        .page_info
        .end_cursor
        .as_deref()
        .is_some_and(|cursor| !cursor.trim().is_empty());
    if page.page_info.has_next_page && !has_cursor {
        return Err(malformed(
            "it said there was a next page but gave no way to reach it",
        ));
    }
    let described = page
        .edges
        .iter()
        .flatten()
        .all(|edge| edge.as_ref().is_some_and(|edge| edge.node.is_some()));
    if !described {
        return Err(malformed("it listed a product without describing it"));
    }
    Ok(())
}

/// An answer from the Commerce API that cannot be used, with what is wrong
/// described by gddy and never quoted from the answer.
fn malformed(what: &str) -> CliCoreError {
    GddyError::unexpected(format!(
        "the Commerce API sent an answer that cannot be used: {what}"
    ))
    .with_fix(NOT_FIXABLE_BY_INPUT)
    .with_system(SYSTEM)
    .into_cli_error()
}

fn config_error(error: impl std::fmt::Display) -> CliCoreError {
    GddyError::config(format!("failed to set up the Commerce client: {error}"))
        .with_system(SYSTEM)
        .into_cli_error()
}

fn network_error(error: reqwest::Error) -> CliCoreError {
    GddyError::network(error.to_string())
        .with_system(SYSTEM)
        .into_cli_error()
}

#[cfg(test)]
mod tests;
