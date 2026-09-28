//! OpenID Connect `userinfo` lookup — resolves *who* a cached OAuth
//! credential belongs to, for `gddy auth status` / `gddy auth login`.
//!
//! # Why a live call rather than a token claim
//!
//! GoDaddy's access tokens deliberately carry no PII: the only user
//! identifier in the JWT is `sub = customer:<uuid>`, which is meaningless to
//! a customer (it isn't shown anywhere in the account UI and can't be used to
//! sign in). With the [`crate::scopes::OPENID`] + [`crate::scopes::PROFILE`]
//! scopes the authorization server *also* issues an `id_token` on the
//! `/token` response and unlocks `GET /v2/oauth2/userinfo`, both of which
//! carry the standard OIDC profile claims (`preferred_username`, `name`,
//! `locale`, `zoneinfo`, …) plus GoDaddy's non-standard `shopperId`.
//!
//! The CLI reads that profile from `/userinfo` with the access token instead
//! of persisting the `id_token`, so the keychain entry keeps holding only the
//! access/refresh tokens and no PII lands on disk — `auth status` fetches it
//! fresh each time it's asked, and nothing is cached here. (cli-engine's PKCE
//! provider also doesn't surface `id_token` from the token response today, so
//! this is the only route available without a framework change.)
//!
//! Failures are never fatal: a network error, a token that predates the OIDC
//! scopes (→ `401`/`403`), or an unexpected body simply leaves the
//! credential's original `customer:<uuid>` identity in place, logged at
//! debug level. `auth status` must keep working offline.

use std::time::Duration;

use cli_engine::{CliCoreError, Credential, Result};
use serde::Deserialize;

use crate::http::make_http_client;
use crate::scopes;

/// Upper bound on a single `/userinfo` round-trip. `auth status` calls this
/// once per cached environment, so a hung network must not turn a local
/// status check into a multi-second stall.
const TIMEOUT: Duration = Duration::from_secs(5);

/// The subset of OIDC `userinfo` claims the CLI consumes. Everything is
/// optional — the authorization server only returns claims covered by the
/// scopes actually granted — and unknown claims (`email`, `phone_number`, …)
/// are ignored rather than modelled, so widening the requested scopes later
/// doesn't silently start capturing more PII here.
#[derive(Clone, Default, Deserialize, PartialEq, Eq)]
pub struct UserInfo {
    /// Subject — matches the access token's `sub` (`customer:<uuid>`).
    #[serde(default)]
    pub sub: String,
    /// GoDaddy shopper ID — the customer-facing account number, usable as a
    /// username at sso.godaddy.com. Not a standard OIDC claim; added under the
    /// `profile` scope. Accepts either casing in case the claim is ever
    /// normalized to snake_case.
    #[serde(default, rename = "shopperId", alias = "shopper_id")]
    pub shopper_id: String,
    /// Standard OIDC `profile` claim: the login/display username.
    #[serde(default)]
    pub preferred_username: String,
    /// Standard OIDC `profile` claim: full display name.
    #[serde(default)]
    pub name: String,
    /// Standard OIDC `profile` claim, e.g. `en-US`.
    #[serde(default)]
    pub locale: String,
    /// Standard OIDC `profile` claim, e.g. `America/Phoenix`.
    #[serde(default)]
    pub zoneinfo: String,
}

/// Hand-written so an accidental `{userinfo:?}` in a log line can't leak the
/// profile: only presence is reported, never the values.
impl std::fmt::Debug for UserInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redact = |value: &str| {
            if value.is_empty() {
                "<none>"
            } else {
                "[redacted]"
            }
        };
        f.debug_struct("UserInfo")
            .field("sub", &redact(&self.sub))
            .field("shopper_id", &redact(&self.shopper_id))
            .field("preferred_username", &redact(&self.preferred_username))
            .field("name", &redact(&self.name))
            .field("locale", &self.locale)
            .field("zoneinfo", &self.zoneinfo)
            .finish()
    }
}

impl UserInfo {
    /// The human-readable identity to show for a logged-in credential, in
    /// the same `kind:value` style as the `customer:<uuid>` it replaces:
    ///
    /// - `kperkins (shopper:123456789)` when both username and shopper ID
    ///   are present,
    /// - `kperkins` or `shopper:123456789` when only one is,
    /// - `None` when the profile carried neither, so the caller keeps the
    ///   credential's existing identity.
    #[must_use]
    pub fn display_identity(&self) -> Option<String> {
        let username = non_blank(&self.preferred_username);
        let shopper = non_blank(&self.shopper_id);
        match (username, shopper) {
            (Some(username), Some(shopper)) => Some(format!("{username} (shopper:{shopper})")),
            (Some(username), None) => Some(username.to_owned()),
            (None, Some(shopper)) => Some(format!("shopper:{shopper}")),
            (None, None) => None,
        }
    }
}

fn non_blank(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

/// `GET {userinfo_url}` with `access_token` as a bearer token.
///
/// # Errors
///
/// Any transport failure, non-2xx status (a token minted without `openid`
/// gets `401`/`403` here), or undecodable body.
pub async fn fetch(userinfo_url: &str, access_token: &str) -> Result<UserInfo> {
    let response = make_http_client()
        .get(userinfo_url)
        .bearer_auth(access_token)
        .header(reqwest::header::ACCEPT, "application/json")
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|err| CliCoreError::message(format!("userinfo request failed: {err}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(CliCoreError::message(format!(
            "userinfo endpoint returned {status}"
        )));
    }
    response.json::<UserInfo>().await.map_err(|err| {
        CliCoreError::message(format!("userinfo response was not valid JSON: {err}"))
    })
}

/// Replaces `credential.identity` with the `/userinfo`-derived username and
/// shopper ID when that's possible; otherwise leaves the credential untouched.
///
/// Skips the network entirely when there's nothing to gain: no token, an
/// expired token (the endpoint would just reject it — and `auth status`
/// reports expiry separately), or a token whose granted scopes are known and
/// don't include `openid` (i.e. a session from before the OIDC scopes became
/// login defaults; the user sees the old `customer:<uuid>` until they
/// `gddy auth login` again). An *unknown* scope set (empty `scopes`) is
/// attempted and left to the server to accept or refuse.
pub async fn enrich_credential(userinfo_url: &str, credential: &mut Credential) {
    if credential.token.is_empty() || credential.is_expired() {
        return;
    }
    if !credential.scopes.is_empty() && !credential.scopes.iter().any(|s| s == scopes::OPENID) {
        tracing::debug!(
            env = %credential.env,
            "cached token was granted without the `openid` scope; \
             re-run `gddy auth login` to show the shopper ID in `auth status`"
        );
        return;
    }
    match fetch(userinfo_url, &credential.token).await {
        Ok(info) => {
            if let Some(identity) = info.display_identity() {
                credential.identity = identity;
            } else {
                tracing::debug!(
                    env = %credential.env,
                    "userinfo response carried no username or shopper ID; keeping token identity"
                );
            }
        }
        Err(err) => {
            tracing::debug!(
                env = %credential.env,
                error = %err,
                "could not resolve userinfo; keeping token identity"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use httpmock::prelude::*;
    use serde_json::json;

    use super::*;

    fn credential(token: &str, scopes: &[&str]) -> Credential {
        Credential {
            token: token.to_owned(),
            env: "prod".to_owned(),
            provider: "godaddy".to_owned(),
            identity: "customer:aa2b50e8-c1d0-40ad-a465-2f53652bad8e".to_owned(),
            sub: "customer:aa2b50e8-c1d0-40ad-a465-2f53652bad8e".to_owned(),
            expires_at: "2999-01-01T00:00:00Z".to_owned(),
            scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
            refreshable: true,
            ..Credential::default()
        }
    }

    #[test]
    fn display_identity_combines_username_and_shopper_id() {
        let info = UserInfo {
            preferred_username: "kperkins".to_owned(),
            shopper_id: "123456789".to_owned(),
            ..UserInfo::default()
        };
        assert_eq!(
            info.display_identity().as_deref(),
            Some("kperkins (shopper:123456789)")
        );
    }

    #[test]
    fn display_identity_falls_back_to_whichever_claim_is_present() {
        let only_shopper = UserInfo {
            shopper_id: " 123456789 ".to_owned(),
            ..UserInfo::default()
        };
        assert_eq!(
            only_shopper.display_identity().as_deref(),
            Some("shopper:123456789")
        );
        let only_username = UserInfo {
            preferred_username: "kperkins".to_owned(),
            ..UserInfo::default()
        };
        assert_eq!(
            only_username.display_identity().as_deref(),
            Some("kperkins")
        );
        assert_eq!(UserInfo::default().display_identity(), None);
        let blank = UserInfo {
            preferred_username: "  ".to_owned(),
            shopper_id: String::new(),
            ..UserInfo::default()
        };
        assert_eq!(blank.display_identity(), None);
    }

    #[test]
    fn deserializes_godaddy_camel_case_shopper_id_and_ignores_unknown_claims() {
        let info: UserInfo = serde_json::from_value(json!({
            "sub": "customer:aa2b50e8-c1d0-40ad-a465-2f53652bad8e",
            "shopperId": "123456789",
            "preferred_username": "kperkins",
            "name": "Ken Perkins",
            "locale": "en-US",
            "zoneinfo": "America/Phoenix",
            "email": "not-modelled@example.test",
            "updated_at": 1_790_634_100
        }))
        .expect("valid userinfo");
        assert_eq!(info.shopper_id, "123456789");
        assert_eq!(info.preferred_username, "kperkins");
        assert_eq!(info.locale, "en-US");

        let snake: UserInfo =
            serde_json::from_value(json!({ "shopper_id": "42" })).expect("valid userinfo");
        assert_eq!(snake.shopper_id, "42");
    }

    #[test]
    fn debug_output_redacts_profile_values() {
        let info = UserInfo {
            preferred_username: "kperkins".to_owned(),
            shopper_id: "123456789".to_owned(),
            name: "Ken Perkins".to_owned(),
            ..UserInfo::default()
        };
        let rendered = format!("{info:?}");
        assert!(!rendered.contains("kperkins"));
        assert!(!rendered.contains("123456789"));
        assert!(!rendered.contains("Ken Perkins"));
        assert!(rendered.contains("[redacted]"));
    }

    #[tokio::test]
    async fn enrich_replaces_identity_with_username_and_shopper_id() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v2/oauth2/userinfo")
                    .header("authorization", "Bearer access-abc")
                    .header("accept", "application/json");
                then.status(200).json_body(json!({
                    "sub": "customer:aa2b50e8-c1d0-40ad-a465-2f53652bad8e",
                    "shopperId": "123456789",
                    "preferred_username": "kperkins"
                }));
            })
            .await;

        let mut credential = credential("access-abc", &["openid", "profile", "offline_access"]);
        enrich_credential(&server.url("/v2/oauth2/userinfo"), &mut credential).await;

        mock.assert_async().await;
        assert_eq!(credential.identity, "kperkins (shopper:123456789)");
        // The subject is left alone: it's still the stable machine identifier.
        assert_eq!(
            credential.sub,
            "customer:aa2b50e8-c1d0-40ad-a465-2f53652bad8e"
        );
    }

    #[tokio::test]
    async fn enrich_keeps_identity_when_endpoint_rejects_token() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET).path("/v2/oauth2/userinfo");
                then.status(401)
                    .json_body(json!({ "error": "invalid_token" }));
            })
            .await;

        // Empty scopes = unknown; we try, and the server's refusal is not fatal.
        let mut credential = credential("access-abc", &[]);
        let before = credential.identity.clone();
        enrich_credential(&server.url("/v2/oauth2/userinfo"), &mut credential).await;

        mock.assert_async().await;
        assert_eq!(credential.identity, before);
    }

    #[tokio::test]
    async fn enrich_keeps_identity_when_profile_has_no_usable_claims() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/v2/oauth2/userinfo");
                then.status(200)
                    .json_body(json!({ "sub": "customer:aa2b50e8-c1d0-40ad-a465-2f53652bad8e" }));
            })
            .await;

        let mut credential = credential("access-abc", &["openid"]);
        let before = credential.identity.clone();
        enrich_credential(&server.url("/v2/oauth2/userinfo"), &mut credential).await;
        assert_eq!(credential.identity, before);
    }

    #[tokio::test]
    async fn enrich_skips_network_for_tokens_without_openid_scope() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET).path("/v2/oauth2/userinfo");
                then.status(200)
                    .json_body(json!({ "shopperId": "123456789" }));
            })
            .await;

        // A pre-OIDC session: scopes are known and lack `openid`.
        let mut credential = credential("access-abc", &["domains.domain:read", "offline_access"]);
        let before = credential.identity.clone();
        enrich_credential(&server.url("/v2/oauth2/userinfo"), &mut credential).await;

        assert_eq!(mock.calls_async().await, 0);
        assert_eq!(credential.identity, before);
    }

    #[tokio::test]
    async fn enrich_skips_network_for_expired_or_tokenless_credentials() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET).path("/v2/oauth2/userinfo");
                then.status(200)
                    .json_body(json!({ "shopperId": "123456789" }));
            })
            .await;

        let mut expired = credential("access-abc", &["openid"]);
        expired.expires_at = "2000-01-01T00:00:00Z".to_owned();
        let before = expired.identity.clone();
        enrich_credential(&server.url("/v2/oauth2/userinfo"), &mut expired).await;
        assert_eq!(expired.identity, before);

        let mut tokenless = credential("", &["openid"]);
        enrich_credential(&server.url("/v2/oauth2/userinfo"), &mut tokenless).await;
        assert_eq!(tokenless.identity, before);

        assert_eq!(mock.calls_async().await, 0);
    }
}
