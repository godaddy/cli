//! DevX Core client for native Android application drafts.

use reqwest::RequestBuilder;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

const NATIVE_APPS_PATH: &str = "/api/v1/native-apps";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeAppInput {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) support_email: String,
    pub(crate) app_category: String,
    pub(crate) merchant_category: String,
    pub(crate) android_package_name: String,
    pub(crate) status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeApp {
    pub(crate) application_id: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) support_email: String,
    pub(crate) app_category: String,
    pub(crate) merchant_category: String,
    pub(crate) android_package_name: String,
    pub(crate) status: String,
    pub(crate) released: bool,
    pub(crate) version_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpsertOperation {
    Created,
    Updated,
}

impl UpsertOperation {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Updated => "updated",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum NativeAppClientError {
    #[error("DevX Core request failed: {0}")]
    Network(#[from] reqwest::Error),
    #[error("DevX Core returned HTTP {status}: {code}{message}")]
    Api {
        status: u16,
        code: String,
        message: DisplayMessage,
    },
    #[error("DevX Core returned an invalid HTTP {status} response: {message}")]
    InvalidResponse { status: u16, message: String },
}

#[derive(Debug)]
pub(crate) struct DisplayMessage(Option<String>);

impl std::fmt::Display for DisplayMessage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(message) = &self.0 {
            write!(formatter, ": {message}")
        } else {
            Ok(())
        }
    }
}

impl From<NativeAppClientError> for crate::error::GddyError {
    fn from(error: NativeAppClientError) -> Self {
        match error {
            NativeAppClientError::Network(error) => {
                Self::network(format!("DevX Core request failed: {error}"))
                    .with_system("applications")
            }
            NativeAppClientError::Api {
                status,
                code,
                message,
            } => Self::from_http(status, format!("{code}{message}"), "applications"),
            NativeAppClientError::InvalidResponse { status, message } => Self::from_http(
                status,
                format!("invalid DevX Core response: {message}"),
                "applications",
            ),
        }
    }
}

#[derive(Debug, Deserialize)]
struct SuccessEnvelope<T> {
    success: bool,
    data: T,
}

#[derive(Debug, Deserialize)]
struct ErrorEnvelope {
    error: ErrorPayload,
}

#[derive(Debug, Deserialize)]
struct ErrorPayload {
    code: String,
    message: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateNativeAppInput<'a> {
    organization_id: &'a str,
    #[serde(flatten)]
    native_app: &'a NativeAppInput,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SupportEmailInput<'a> {
    support_email: &'a str,
}

pub(crate) struct NativeAppClient {
    base_url: String,
    token: String,
    http: reqwest::Client,
}

impl NativeAppClient {
    pub(crate) fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            token: token.into(),
            http: crate::application::client::make_http_client(),
        }
    }

    pub(crate) async fn get(
        &self,
        application_id: &str,
    ) -> Result<Option<NativeApp>, NativeAppClientError> {
        let request = self
            .http
            .get(self.url(application_id))
            .bearer_auth(&self.token);
        self.send(request).await
    }

    pub(crate) async fn create(
        &self,
        application_id: &str,
        organization_id: &str,
        input: &NativeAppInput,
    ) -> Result<NativeApp, NativeAppClientError> {
        let request = self
            .http
            .post(self.url(application_id))
            .bearer_auth(&self.token)
            .json(&CreateNativeAppInput {
                organization_id,
                native_app: input,
            });
        self.send(request).await
    }

    pub(crate) async fn update(
        &self,
        application_id: &str,
        input: &NativeAppInput,
    ) -> Result<NativeApp, NativeAppClientError> {
        let request = self
            .http
            .patch(self.url(application_id))
            .bearer_auth(&self.token)
            .json(input);
        self.send(request).await
    }

    async fn update_support_email(
        &self,
        application_id: &str,
        support_email: &str,
    ) -> Result<NativeApp, NativeAppClientError> {
        let request = self
            .http
            .patch(self.url(application_id))
            .bearer_auth(&self.token)
            .json(&SupportEmailInput { support_email });
        self.send(request).await
    }

    pub(crate) async fn upsert(
        &self,
        application_id: &str,
        organization_id: &str,
        input: &NativeAppInput,
    ) -> Result<UpsertOperation, NativeAppClientError> {
        if self.get(application_id).await?.is_some() {
            self.update(application_id, input).await?;
            return Ok(UpsertOperation::Updated);
        }

        self.create(application_id, organization_id, input).await?;
        // DevX Portal currently follows create with this patch because
        // application-service does not reliably persist supportEmail on create.
        self.update_support_email(application_id, &input.support_email)
            .await?;
        Ok(UpsertOperation::Created)
    }

    fn url(&self, application_id: &str) -> String {
        let encoded_id: String =
            url::form_urlencoded::byte_serialize(application_id.as_bytes()).collect();
        format!("{}{NATIVE_APPS_PATH}/{encoded_id}", self.base_url)
    }

    async fn send<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
    ) -> Result<T, NativeAppClientError> {
        let request = request.build()?;
        cli_engine::transport::debug_log_reqwest_request(&request);
        let response = self.http.execute(request).await?;
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.bytes().await?;
        cli_engine::transport::debug_log_reqwest_response(status, &headers, &bytes);

        if !status.is_success() {
            return Err(api_error(status.as_u16(), &bytes));
        }

        let envelope: SuccessEnvelope<T> = serde_json::from_slice(&bytes).map_err(|error| {
            NativeAppClientError::InvalidResponse {
                status: status.as_u16(),
                message: error.to_string(),
            }
        })?;
        if !envelope.success {
            return Err(NativeAppClientError::InvalidResponse {
                status: status.as_u16(),
                message: "success envelope contained success=false".to_owned(),
            });
        }
        Ok(envelope.data)
    }
}

fn api_error(status: u16, body: &[u8]) -> NativeAppClientError {
    if let Ok(envelope) = serde_json::from_slice::<ErrorEnvelope>(body) {
        return NativeAppClientError::Api {
            status,
            code: envelope.error.code,
            message: DisplayMessage(envelope.error.message),
        };
    }

    NativeAppClientError::Api {
        status,
        code: String::from_utf8_lossy(body).into_owned(),
        message: DisplayMessage(None),
    }
}

#[cfg(test)]
mod tests {
    use httpmock::{Method, MockServer};
    use serde_json::json;

    use super::*;

    fn input() -> NativeAppInput {
        NativeAppInput {
            name: "Example Native App".to_owned(),
            description: "Example description".to_owned(),
            support_email: "support@example.com".to_owned(),
            app_category: String::new(),
            merchant_category: String::new(),
            android_package_name: "com.example.app".to_owned(),
            status: "draft".to_owned(),
        }
    }

    fn native_app_json() -> serde_json::Value {
        json!({
            "applicationId": "app-1",
            "name": "Example Native App",
            "description": "Example description",
            "supportEmail": "support@example.com",
            "appCategory": "",
            "merchantCategory": "",
            "androidPackageName": "com.example.app",
            "status": "draft",
            "released": false
        })
    }

    #[tokio::test]
    async fn get_sends_bearer_user_agent_and_decodes_success() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(Method::GET)
                    .path("/api/v1/native-apps/app-1")
                    .header("authorization", "Bearer test-token")
                    .header(
                        "user-agent",
                        concat!("godaddy-cli/", env!("CARGO_PKG_VERSION")),
                    );
                then.status(200).json_body(json!({
                    "success": true,
                    "data": native_app_json()
                }));
            })
            .await;

        let app = NativeAppClient::new(server.base_url(), "test-token")
            .get("app-1")
            .await
            .expect("get native app")
            .expect("native app exists");

        mock.assert_async().await;
        assert_eq!(app.application_id, "app-1");
        assert_eq!(app.android_package_name, "com.example.app");
    }

    #[tokio::test]
    async fn upsert_creates_when_absent_and_patches_support_email() {
        let server = MockServer::start_async().await;
        let get = server
            .mock_async(|when, then| {
                when.method(Method::GET).path("/api/v1/native-apps/app-1");
                then.status(200)
                    .json_body(json!({ "success": true, "data": null }));
            })
            .await;
        let create = server
            .mock_async(|when, then| {
                when.method(Method::POST)
                    .path("/api/v1/native-apps/app-1")
                    .header("authorization", "Bearer test-token")
                    .json_body(json!({
                        "organizationId": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Example Native App",
                        "description": "Example description",
                        "supportEmail": "support@example.com",
                        "appCategory": "",
                        "merchantCategory": "",
                        "androidPackageName": "com.example.app",
                        "status": "draft"
                    }));
                then.status(200).json_body(json!({
                    "success": true,
                    "data": native_app_json()
                }));
            })
            .await;
        let support_patch = server
            .mock_async(|when, then| {
                when.method(Method::PATCH)
                    .path("/api/v1/native-apps/app-1")
                    .json_body(json!({ "supportEmail": "support@example.com" }));
                then.status(200).json_body(json!({
                    "success": true,
                    "data": native_app_json()
                }));
            })
            .await;

        let operation = NativeAppClient::new(server.base_url(), "test-token")
            .upsert("app-1", "550e8400-e29b-41d4-a716-446655440000", &input())
            .await
            .expect("create native app");

        assert_eq!(operation, UpsertOperation::Created);
        get.assert_async().await;
        create.assert_async().await;
        support_patch.assert_async().await;
    }

    #[tokio::test]
    async fn upsert_updates_when_present() {
        let server = MockServer::start_async().await;
        let get = server
            .mock_async(|when, then| {
                when.method(Method::GET).path("/api/v1/native-apps/app-1");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": native_app_json()
                }));
            })
            .await;
        let update = server
            .mock_async(|when, then| {
                when.method(Method::PATCH)
                    .path("/api/v1/native-apps/app-1")
                    .json_body(json!({
                        "name": "Example Native App",
                        "description": "Example description",
                        "supportEmail": "support@example.com",
                        "appCategory": "",
                        "merchantCategory": "",
                        "androidPackageName": "com.example.app",
                        "status": "draft"
                    }));
                then.status(200).json_body(json!({
                    "success": true,
                    "data": native_app_json()
                }));
            })
            .await;

        let operation = NativeAppClient::new(server.base_url(), "test-token")
            .upsert("app-1", "ignored-org", &input())
            .await
            .expect("update native app");

        assert_eq!(operation, UpsertOperation::Updated);
        get.assert_async().await;
        update.assert_async().await;
    }

    #[tokio::test]
    async fn error_envelope_preserves_service_code_and_message() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(Method::PATCH).path("/api/v1/native-apps/app-1");
                then.status(409).json_body(json!({
                    "success": false,
                    "error": {
                        "code": "PACKAGE_NAME_IMMUTABLE",
                        "message": "Package name cannot change after release"
                    }
                }));
            })
            .await;

        let error = NativeAppClient::new(server.base_url(), "test-token")
            .update("app-1", &input())
            .await
            .expect_err("immutable package name must fail");
        let message = error.to_string();
        assert!(message.contains("PACKAGE_NAME_IMMUTABLE"), "{message}");
        assert!(message.contains("cannot change after release"), "{message}");
    }

    #[tokio::test]
    async fn malformed_success_envelope_is_rejected() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(Method::GET).path("/api/v1/native-apps/app-1");
                then.status(200)
                    .json_body(json!({ "success": true, "data": { "unexpected": true } }));
            })
            .await;

        let error = NativeAppClient::new(server.base_url(), "test-token")
            .get("app-1")
            .await
            .expect_err("invalid native app response must fail");
        assert!(matches!(
            error,
            NativeAppClientError::InvalidResponse { status: 200, .. }
        ));
    }

    #[test]
    fn url_encodes_application_id_path_segment() {
        let client = NativeAppClient::new("https://example.test/", "token");
        assert_eq!(
            client.url("app/with space"),
            "https://example.test/api/v1/native-apps/app%2Fwith+space"
        );
    }
}
