//! Generated App Registry GraphQL client
//!
//! The contents of this crate are **generated** by `graphql_client_codegen`
//! at build time from the vendored subgraph schema (`graphql/schema.graphql`)
//! and this crate's `.graphql` operation documents. Construct [`Client`] with
//! [`client_with_auth`] to supply a pre-authenticated `reqwest::Client`
//! wrapper (the CLI sets the `Authorization: Bearer <token>` header itself),
//! then call [`Client::send`] with any generated operation type (it
//! implements `graphql_client::GraphQLQuery`).
//!
//! The lint allowances are scoped to the generated module so the hand-written
//! code below is still linted normally. `BuildError`/`TransportObserver`/
//! `set_transport_observer` are shared with every other generated client
//! crate — see `generated-client-support`.

/// Rust types for this schema's custom scalars, referenced by the generated
/// module via `set_custom_scalars_module` in `build.rs`.
#[allow(clippy::upper_case_acronyms)]
pub mod scalars {
    /// The schema declares no format for `DateTime` beyond "a string" — kept
    /// as the wire string rather than parsed, matching how
    /// `ApplicationClient` treats every other field today (opaque
    /// `serde_json::Value` passthrough to `CommandResult`).
    pub type DateTime = String;
    /// Named to match the schema's own scalar name exactly —
    /// `custom_scalars_module` codegen references `crate::scalars::<name>`
    /// verbatim, casing included.
    pub type JSONObject = serde_json::Value;
    pub type Null = ();
}

/// graphql_client-generated operation types. Exempt from the workspace's
/// strict style/rustdoc lints (it's machine-generated); the rest of the
/// crate is not.
mod generated {
    #![allow(clippy::all)]
    #![allow(dead_code)]
    #![allow(unused_imports)]
    #![allow(rustdoc::all)]

    use crate::scalars::{DateTime, JSONObject, Null};

    include!(concat!(env!("OUT_DIR"), "/codegen.rs"));
}

pub use generated::*;
pub use generated_client_support::{BuildError, TransportObserver, set_transport_observer};

const GRAPHQL_PATH: &str = "/v1/apps/app-registry-subgraph";

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("HTTP error {status}: {body}")]
    Http { status: u16, body: String },
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("GraphQL errors: {0}")]
    GraphQL(String),
}

/// Wraps a pre-authenticated `reqwest::Client` and sends any generated
/// operation (a `graphql_client::GraphQLQuery` implementor) to the App
/// Registry subgraph.
pub struct Client {
    http: reqwest::Client,
    base_url: String,
}

/// Build a [`Client`] whose every request carries a pre-set `Authorization`
/// header and `x-request-id`. `authorization` is the full header value the
/// App Registry endpoint expects — e.g. `"Bearer <token>"`.
pub fn client_with_auth(
    base_url: &str,
    authorization: &str,
    user_agent: &str,
    request_id: &str,
) -> Result<Client, BuildError> {
    let http = generated_client_support::build_authenticated_http_client(
        authorization,
        user_agent,
        request_id,
    )?;
    Ok(Client {
        http,
        base_url: base_url.to_owned(),
    })
}

impl Client {
    /// Sends `Q` (any generated operation type) and returns its typed
    /// response data — or a [`ClientError::GraphQL`] built from the
    /// response's `errors` array, the same "HTTP 200 with a top-level
    /// `errors` array" convention `platform::app::client::ClientError`
    /// hand-rolls today.
    pub async fn send<Q>(&self, variables: Q::Variables) -> Result<Q::ResponseData, ClientError>
    where
        Q: graphql_client::GraphQLQuery,
    {
        let body = Q::build_query(variables);
        let url = format!("{}{GRAPHQL_PATH}", self.base_url);
        let request = self.http.post(&url).json(&body).build()?;
        generated_client_support::notify_request(&request);

        let result = self.http.execute(request).await;
        generated_client_support::notify_response_result(&result);
        let response = result?;

        let status = response.status();
        let bytes = response.bytes().await?;
        if !status.is_success() {
            return Err(ClientError::Http {
                status: status.as_u16(),
                body: String::from_utf8_lossy(&bytes).into_owned(),
            });
        }

        let parsed: graphql_client::Response<Q::ResponseData> = serde_json::from_slice(&bytes)
            .map_err(|e| ClientError::Http {
                status: status.as_u16(),
                body: format!(
                    "invalid JSON response: {e} (body: {})",
                    String::from_utf8_lossy(&bytes)
                ),
            })?;
        if let Some(errors) = parsed.errors.filter(|errors| !errors.is_empty()) {
            return Err(ClientError::GraphQL(format!("{errors:?}")));
        }
        parsed
            .data
            .ok_or_else(|| ClientError::GraphQL("response had no data and no errors".to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use httpmock::prelude::*;
    use serde_json::json;

    use super::*;

    fn client_for(server: &MockServer) -> Client {
        client_with_auth(
            &server.base_url(),
            "Bearer test-token",
            "test-agent",
            "req-1",
        )
        .expect("build client")
    }

    #[tokio::test]
    async fn applications_list_sends_all_app_registry_statuses() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/apps/app-registry-subgraph")
                    .header("authorization", "Bearer test-token")
                    .is_true(|req| {
                        let body = req.body_string();
                        body.contains("ApplicationsList")
                            && body.contains(
                                r#""in":["ACTIVE","ARCHIVED","BLOCKED","INACTIVE","VERIFYING"]"#,
                            )
                    });
                then.status(200).json_body(json!({
                    "data": {
                        "applications": {
                            "edges": [
                                { "node": { "id": "a1", "label": "Active", "name": "active-app", "description": null, "status": "ACTIVE", "url": null, "proxyUrl": null } },
                                { "node": { "id": "a2", "label": "Inactive", "name": "inactive-app", "description": null, "status": "INACTIVE", "url": null, "proxyUrl": null } }
                            ]
                        }
                    }
                }));
            })
            .await;

        let statuses = ["ACTIVE", "ARCHIVED", "BLOCKED", "INACTIVE", "VERIFYING"]
            .into_iter()
            .map(|s| serde_json::from_value(json!(s)).expect("valid ApplicationStatus"))
            .collect();
        let variables = applications_list::Variables {
            status: Some(applications_list::ApplicationStatusFilter {
                eq: None,
                in_: Some(statuses),
                ne: None,
            }),
        };

        let data = client_for(&server)
            .send::<ApplicationsList>(variables)
            .await
            .expect("list applications");

        mock.assert_async().await;
        let edges = data.applications.expect("applications connection").edges;
        assert_eq!(edges.expect("edges").len(), 2);
    }

    #[tokio::test]
    async fn activate_release_posts_mutation_with_ids() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/apps/app-registry-subgraph")
                    .header("authorization", "Bearer test-token")
                    .is_true(|req| {
                        let body = req.body_string();
                        body.contains("activateRelease")
                            && body.contains("app-123")
                            && body.contains("rel-456")
                    });
                then.status(200).json_body(json!({
                    "data": {
                        "activateRelease": {
                            "id": "rel-456",
                            "version": "1.0.0",
                            "description": null,
                            "status": "ACTIVE",
                            "activatedAt": "2026-09-18T00:00:00Z",
                            "createdAt": "2026-09-18T00:00:00Z",
                            "updatedAt": "2026-09-18T00:00:00Z"
                        }
                    }
                }));
            })
            .await;

        let variables = activate_release::Variables {
            application_id: "app-123".to_owned(),
            release_id: "rel-456".to_owned(),
        };

        let data = client_for(&server)
            .send::<ActivateRelease>(variables)
            .await
            .expect("activate release");

        mock.assert_async().await;
        let release = data.activate_release.expect("activateRelease result");
        assert_eq!(release.id, "rel-456");
        assert_eq!(release.version, "1.0.0");
    }

    /// Same "GraphQL errors on HTTP 200" convention
    /// `platform::app::client::ClientError::GraphQL` hand-rolls today —
    /// proves the generic `graphql_client::Response<T>` wrapper covers it
    /// without extra code on our side.
    #[tokio::test]
    async fn send_surfaces_graphql_errors_on_http_200() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/apps/app-registry-subgraph");
                then.status(200).json_body(json!({
                    "errors": [{ "message": "release not found" }]
                }));
            })
            .await;

        let variables = activate_release::Variables {
            application_id: "app-123".to_owned(),
            release_id: "missing".to_owned(),
        };

        let err = client_for(&server)
            .send::<ActivateRelease>(variables)
            .await
            .expect_err("graphql errors should surface");

        mock.assert_async().await;
        assert!(
            matches!(err, ClientError::GraphQL(ref msg) if msg.contains("release not found")),
            "expected GraphQL error variant, got: {err:?}"
        );
    }
}
