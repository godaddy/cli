//! Generated App Registry GraphQL client
//!
//! The contents of this crate are **generated** by `graphql_client_codegen`
//! at build time from the vendored subgraph schema (`graphql/schema.graphql`)
//! and this crate's `.graphql` operation documents. Construct [`Client`] with
//! [`client_with_builder`] to supply a pre-authenticated `reqwest::Client`
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

/// Operations whose responses carry credentials: `CreateApplication` returns
/// the client secret, webhook secret and public key, and
/// `GenerateReleaseUploadUrl` returns a pre-signed upload URL. Their response
/// bodies are never handed to the transport observer, so `--debug transport`
/// cannot write them into a trace, and a response that fails to decode is
/// reported without its contents so they cannot reach an error message either.
const SECRET_RESPONSE_OPERATIONS: &[&str] = &["CreateApplication", "GenerateReleaseUploadUrl"];

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

/// Builds a [`Client`] whose every request carries a pre-set `Authorization`
/// header and `x-request-id`.
///
/// `builder` carries the host application's shared transport policy
/// (user-agent, timeouts, default headers); `authorization` is the full header
/// value the App Registry endpoint expects, e.g. `"Bearer <token>"`. The
/// builder is a `reqwest` type, so the caller's `reqwest` must resolve to the
/// same version as this crate's.
///
/// # Errors
///
/// [`BuildError`] if `base_url` is not HTTPS (plain HTTP is allowed only on a
/// loopback host), if a header value is invalid, or if the client cannot be
/// built.
pub fn client_with_builder(
    builder: reqwest::ClientBuilder,
    base_url: &str,
    authorization: &str,
    request_id: &str,
) -> Result<Client, BuildError> {
    generated_client_support::validate_base_url(base_url)?;
    let http = generated_client_support::build_authenticated_http_client(
        builder,
        authorization,
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

        let carries_credentials = SECRET_RESPONSE_OPERATIONS.contains(&body.operation_name);
        let response = if carries_credentials {
            self.http.execute(request).await?
        } else {
            generated_client_support::execute_and_observe(&self.http, request).await?
        };

        let status = response.status();
        let bytes = response.bytes().await?;
        if !status.is_success() {
            return Err(ClientError::Http {
                status: status.as_u16(),
                body: if carries_credentials {
                    "response body withheld: the operation returns credentials".to_owned()
                } else {
                    String::from_utf8_lossy(&bytes).into_owned()
                },
            });
        }

        let parsed: graphql_client::Response<Q::ResponseData> = serde_json::from_slice(&bytes)
            .map_err(|e| ClientError::Http {
                status: status.as_u16(),
                // serde's message can quote offending values, so a response
                // that carries credentials is described by position only.
                body: if carries_credentials {
                    format!(
                        "invalid JSON response at line {} column {} (details withheld: \
                         the response carries credentials)",
                        e.line(),
                        e.column()
                    )
                } else {
                    format!(
                        "invalid JSON response: {e} (body: {})",
                        String::from_utf8_lossy(&bytes)
                    )
                },
            })?;
        if let Some(errors) = parsed.errors.filter(|errors| !errors.is_empty()) {
            // `graphql_client::Error` has its own `Display` (`path:line:col:
            // message`) — join that instead of `Debug`-formatting the whole
            // struct, which included noisy `None`/empty `extensions` fields
            // and wasn't stable across schema changes to those fields.
            let joined = errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(ClientError::GraphQL(joined));
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
        client_with_builder(
            reqwest::Client::builder(),
            &server.base_url(),
            "Bearer test-token",
            "req-1",
        )
        .expect("build client")
    }

    #[test]
    fn rejects_a_non_https_base_url_that_is_not_loopback() {
        let error = client_with_builder(
            reqwest::Client::builder(),
            "http://api.example.com",
            "Bearer test-token",
            "req-1",
        )
        .err()
        .expect("plain HTTP to a remote host is refused");
        assert!(matches!(error, BuildError::InsecureBaseUrlScheme { .. }));

        client_with_builder(
            reqwest::Client::builder(),
            "https://api.example.com",
            "Bearer test-token",
            "req-1",
        )
        .expect("https is accepted");
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

    /// Records every response body handed to the transport observer.
    struct RecordingObserver(std::sync::Mutex<Vec<String>>);

    impl TransportObserver for RecordingObserver {
        fn on_request(&self, _request: &reqwest::Request) {}

        fn on_response(
            &self,
            _status: reqwest::StatusCode,
            _headers: &reqwest::header::HeaderMap,
            body: &[u8],
        ) {
            self.0
                .lock()
                .expect("recorder lock")
                .push(String::from_utf8_lossy(body).into_owned());
        }
    }

    #[tokio::test]
    async fn credential_bearing_responses_are_not_shown_to_the_transport_observer() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/apps/app-registry-subgraph")
                    .is_true(|req| req.body_string().contains("CreateApplication"));
                then.status(200).json_body(json!({
                    "data": {
                        "createApplication": {
                            "id": "app-1",
                            "clientId": "client-1",
                            "clientSecret": "client-secret-value",
                            "label": "My App",
                            "name": "my-app",
                            "description": null,
                            "status": "ACTIVE",
                            "url": null,
                            "proxyUrl": null,
                            "authorizationScopes": [],
                            "redirectUris": [],
                            "secret": "webhook-secret-value",
                            "publicKey": "public-key-value"
                        }
                    }
                }));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/apps/app-registry-subgraph")
                    .is_true(|req| req.body_string().contains("observed-app"));
                then.status(200).json_body(json!({
                    "data": {
                        "application": {
                            "id": "app-1",
                            "label": "My App",
                            "name": "observed-app",
                            "description": null,
                            "status": "ACTIVE",
                            "url": null,
                            "proxyUrl": null
                        }
                    }
                }));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/apps/app-registry-subgraph")
                    .is_true(|req| req.body_string().contains("GenerateReleaseUploadUrl"));
                then.status(200).json_body(json!({
                    "data": {
                        "generateReleaseUploadUrl": {
                            "uploadId": "up-1",
                            "url": "https://s3.example.com/upload?X-Amz-Signature=signed-url-secret",
                            "key": "key-1",
                            "expiresAt": "2026-01-01T00:00:00Z",
                            "maxSizeBytes": 1024,
                            "requiredHeaders": []
                        }
                    }
                }));
            })
            .await;

        let observer = std::sync::Arc::new(RecordingObserver(std::sync::Mutex::new(Vec::new())));
        set_transport_observer(Some(observer.clone()));

        let client = client_for(&server);
        let variables: create_application::Variables = serde_json::from_value(json!({
            "input": { "name": "my-app", "label": "My App" }
        }))
        .expect("create variables");
        let created = client
            .send::<CreateApplication>(variables)
            .await
            .expect("create application");
        client
            .send::<Application>(application::Variables {
                name: "observed-app".to_owned(),
            })
            .await
            .expect("get application");
        let upload_variables: generate_release_upload_url::Variables =
            serde_json::from_value(json!({
                "input": { "applicationId": "app-1", "releaseId": "rel-1", "contentType": "JS" }
            }))
            .expect("upload variables");
        let upload = client
            .send::<GenerateReleaseUploadUrl>(upload_variables)
            .await
            .expect("generate upload url");

        set_transport_observer(None);
        assert!(
            upload
                .generate_release_upload_url
                .is_some_and(|u| u.url.contains("signed-url-secret")),
            "the caller still receives the signed URL"
        );
        assert_eq!(
            created.create_application.and_then(|app| app.client_secret),
            Some("client-secret-value".to_owned()),
            "the caller still receives the credentials"
        );
        let seen = observer.0.lock().expect("recorder lock").join("\n");
        assert!(
            seen.contains("observed-app"),
            "ordinary responses are still observed"
        );
        assert!(
            !seen.contains("client-secret-value")
                && !seen.contains("webhook-secret-value")
                && !seen.contains("signed-url-secret"),
            "credential-bearing response leaked to the observer: {seen}"
        );
    }

    /// An error status keeps its code but, for operations that return
    /// credentials, not the body.
    #[tokio::test]
    async fn credential_operations_withhold_error_response_bodies() {
        let server = MockServer::start_async().await;
        // Distinct from the strings in the observer test above: the transport
        // observer is process-wide, so tests running in parallel share it.
        server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/apps/app-registry-subgraph")
                    .is_true(|req| {
                        let body = req.body_string();
                        body.contains("CreateApplication")
                            || body.contains("GenerateReleaseUploadUrl")
                    });
                then.status(500).body("failure with error-body-secret");
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/apps/app-registry-subgraph")
                    .is_true(|req| req.body_string().contains(r#""name":"plain-app""#));
                then.status(500).body("upstream failure");
            })
            .await;
        let client = client_for(&server);

        let create: create_application::Variables = serde_json::from_value(json!({
            "input": { "name": "my-app", "label": "My App" }
        }))
        .expect("create variables");
        let upload: generate_release_upload_url::Variables = serde_json::from_value(json!({
            "input": { "applicationId": "app-1", "releaseId": "rel-1", "contentType": "JS" }
        }))
        .expect("upload variables");
        let errors = [
            client
                .send::<CreateApplication>(create)
                .await
                .expect_err("500 is an error"),
            client
                .send::<GenerateReleaseUploadUrl>(upload)
                .await
                .expect_err("500 is an error"),
        ];

        for error in errors {
            assert!(
                matches!(error, ClientError::Http { status: 500, .. }),
                "{error:?}"
            );
            let shown = format!("{error} {error:?}");
            assert!(
                !shown.contains("error-body-secret"),
                "error body leaked: {shown}"
            );
        }

        // Ordinary operations keep the body for diagnosis.
        let ordinary = client
            .send::<Application>(application::Variables {
                name: "plain-app".to_owned(),
            })
            .await
            .expect_err("500 is an error");
        assert!(
            ordinary.to_string().contains("upstream failure"),
            "{ordinary}"
        );
    }

    /// A response the client cannot decode is reported in the error. For an
    /// operation that returns credentials, serde's message (which can quote the
    /// offending value) and the raw body must stay out of it.
    #[tokio::test]
    async fn undecodable_credential_responses_do_not_leak_into_the_error() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/apps/app-registry-subgraph");
                then.status(200).json_body(json!({
                    "data": {
                        "createApplication": {
                            "id": "app-1",
                            "clientSecret": "decode-secret-value",
                            "secret": "decode-webhook-value",
                            // Wrong type: serde would quote this value in its message.
                            "authorizationScopes": "decode-secret-value"
                        }
                    }
                }));
            })
            .await;

        let variables: create_application::Variables = serde_json::from_value(json!({
            "input": { "name": "my-app", "label": "My App" }
        }))
        .expect("create variables");
        let error = client_for(&server)
            .send::<CreateApplication>(variables)
            .await
            .expect_err("response cannot be decoded");

        let message = error.to_string();
        assert!(message.contains("invalid JSON response"), "{message}");
        assert!(
            !message.contains("decode-secret-value") && !message.contains("decode-webhook-value"),
            "credentials leaked into the error: {message}"
        );
    }
}
