# Proposal: `gddy db tunnel --product wordpress` — MySQL access for Managed WordPress

Status: draft. The CLI, airo-go, and hosting mint changes are implemented but not
merged. The end-to-end flow is **blocked**: Managed WordPress apps do not run an
agent today (see **Blocker: no agent on Managed WordPress**).

This document covers only what differs for Managed WordPress (product `mwp`,
product app type `mhwp`). The WebSocket relay, bind safety, events, feature
gating, and the MySQL security model are the same as for Node.js Hosting; see
[db-tunnel.md](./db-tunnel.md).

## Motivation

`gddy db tunnel` gives a developer a local MySQL port that relays to an app's
database through the app's per-app **agent**. For Node.js Hosting apps, the
CLI gets the agent URL and a short-lived token from the Node.js Hosting API.
Managed WordPress sites are Airo-managed apps: they are owned through an Airo
subscription, and the Node.js Hosting API does not know about them. They need
their own mint path, and the rest of the tunnel stays the same.

## How it works

The two planes are the same as for Node.js:

- **Control plane.** One HTTPS call when the command starts. For WordPress it
  goes to the **Airo API** (airo-go) and not to the Node.js Hosting API.
  airo-go checks the caller and asks hosting to mint.
- **Data plane.** One WebSocket per local TCP connection, straight from the CLI
  to the app's agent, carrying raw MySQL bytes. It never touches airo-go or
  hosting.

```mermaid
sequenceDiagram
    autonumber
    participant CLI as gddy db tunnel<br/>--product wordpress
    participant Route as Public route to airo-go<br/>(not confirmed)
    participant Airo as airo-go<br/>(Airo API)
    participant Host as hosting API
    participant SSO as SSO (cert2s)
    participant AAB as airo-builder<br/>/api/auth/agent-token
    participant Agent as App agent
    participant DB as App MySQL

    CLI->>Route: POST /v1/airo/hosting/apps/{appId}/database-tunnel/agent-token<br/>Authorization: Bearer <OAuth>
    Route->>Airo: POST /v1/hosting/apps/{appId}/database-tunnel/agent-token
    Airo->>Airo: /v1/hosting OAuth chain: validate OAuth JWT (JWKS, iss, aud, typ, exp),<br/>resolve the caller's shopper, TLA gate
    Airo->>Airo: require scope hosting.database.tunnel:execute
    Airo->>Airo: rate limit per app + customer (10/min, burst 5)
    Airo->>Airo: GetCustomerForApp(appId)<br/>caller must be the owner → owner shopperId
    Airo->>Host: POST hosting/v1/apps/{appId}/database-tunnel/agent-token<br/>service JWT, body {shopperId}, X-OAuth-Token
    Host->>Host: resolve agent URL from the composition URL patterns<br/>(https only; none → 404, no mint)
    Host->>SSO: cert2s delegation for shopperId
    Host->>AAB: POST /api/auth/agent-token<br/>Authorization: sso-jwt <cert2s>, X-OAuth-Token<br/>{siteId, owningProduct: AiroAppBuilder, requestDatabaseTunnel: true, ttl 1h}
    AAB-->>Host: {token} (agent JWT with canTunnelDatabase)
    Host->>Host: check token exp (≤ 1h + 1m skew)
    Host-->>Airo: {agentUrl, token, expires}
    Airo-->>CLI: {agentUrl, token, expires} (passed through)
    CLI->>Agent: per TCP conn: WSS /apps/{appId}/database/tunnel<br/>Authorization: Bearer <agent token>
    Agent->>DB: connect(host, port), then relay raw bytes
```

### Step by step

1. **CLI.** `--product wordpress` selects
   `HostingClient::get_airo_database_tunnel_token`. It sends
   `POST {api base}/v1/airo/hosting/apps/{appId}/database-tunnel/agent-token` with the
   CLI's OAuth token. The CLI gets that token with the same scopes as for
   Node.js: deploy-execute and `hosting.database.tunnel:execute`. The response
   body is not logged, because it contains the agent token. The response shape
   `{agentUrl, token}` is the same as for Node.js, so everything after the mint
   is shared code.
2. **Public route.** The request must reach airo-go's
   `/v1/hosting/apps/{appId}/database-tunnel/agent-token` with the
   `Authorization` header unchanged. **How the public URL maps to airo-go is not
   confirmed.** The CLI path above assumes that `/v1/airo/...` on the API host
   maps to airo-go's `/v1/...`. Nothing in this proposal depends on a particular
   proxy in between (see **Cross-team dependencies**).
3. **airo-go: authenticate.** The route is on airo-go's public `/v1/hosting`
   group, which accepts only an Authorization Platform OAuth access token
   (`OAuthAuthMiddleware`, BACK-3991). It never accepts an `sso-jwt`, an admin
   token, or a service certificate. It validates the token the same way the
   PaaS public API (HWA) does:
   - The issuer and its key set are configured (`oauthIssuer`, `oauthJwksUrl`:
     `https://oauth.api.<root>` and `https://api.<root>/v2/oauth2/jwks`). If
     they are not configured, the group, and so this route, is not mounted.
   - The token must use RS256 with a key of at least 2048 bits. The audience
     must be `godaddy.com`, and `typ` either `at+jwt` or `application/at+jwt`.
     `exp` is required, with 30 seconds of leeway. Delegated tokens (an `act`
     claim) are refused.
   - `sub` must be `customer:<uuid>`. The middleware maps that customer to a
     shopper ID (`airo_customers`, then the Shopper API) and applies the TLA
     shopper gate, which is enforced in production.

   A missing, malformed, or invalid token, or an `sso-jwt` header, gets `401`.
   A valid token for a customer with no shopper, or a shopper outside the TLA
   gate, gets `403`. If the signing keys cannot be fetched, the answer is
   `503`.
4. **airo-go: scope and rate limit.** Every `/v1/hosting` route names its
   scope. This one requires `hosting.database.tunnel:execute`; a token without
   it gets `403 Insufficient scope`, so deploy authority alone is not enough.
   Then at most 10 requests per minute with a burst of 5 are allowed, counted
   per app and customer (`ExecutionRateLimitKey`). Beyond that the answer is
   `429`.
5. **airo-go: ownership.** The CLI sends only the app ID. airo-go looks up the
   app's subscription and the customer who owns it (`GetCustomerForApp`), and
   the caller must be that customer. An unknown app, a failed lookup, another
   customer's app, or an owner without a shopper ID all get the same
   `404 app not found`, so the answer does not show whether a foreign app
   exists. Only the owner can open a tunnel. Collaborator grants are not
   resolved on this path.
6. **airo-go → hosting.** airo-go calls the internal hosting mint with its
   service credential. It sends the owner's `shopperId` in the body and the
   caller's OAuth token in `X-OAuth-Token`.
7. **hosting: agent URL first.** hosting resolves the app and its cell and
   builds the app's URLs from its composition. It takes the `agent` URL, from
   the `preview` variant first and then the other variants in alphabetical
   order. If no variant has one, it answers `404 App does not have an agent
   URL` and **does not mint**. If the agent URL is not `https`, it also does
   not mint.
8. **hosting: mint.** hosting gets a cert2s delegation for the shopper. It
   then calls airo-builder's `/api/auth/agent-token` with
   `owningProduct: AiroAppBuilder` and `requestDatabaseTunnel: true`, and asks
   for a token that lives one hour. The token is never cached. hosting reads
   the token's `exp` and refuses to return a token that has already expired or
   that lives longer than 1 hour plus 1 minute.
9. **Relay.** The CLI opens one WebSocket per MySQL connection to
   `wss://{agent host}/apps/{appId}/database/tunnel`. The agent checks the
   token, including the `canTunnelDatabase` claim and that the token's app
   matches the app in the path. It then dials the app's database and relays
   bytes. Everything from here on is described in [db-tunnel.md](./db-tunnel.md).

## Command surface

This adds one flag to the flags in [db-tunnel.md](./db-tunnel.md#command-surface):

| Flag | Required | Default | Purpose |
| --- | --- | --- | --- |
| `--product <PRODUCT>` | no | `nodejs` | Selects the service that mints the tunnel token: `nodejs` uses the Node.js Hosting API, `wordpress` uses the Airo API. Any other value is rejected by the argument parser. |

```console
$ gddy db tunnel --app-id <app-id> --product wordpress
$ mysql --ssl-mode=REQUIRED -h 127.0.0.1 -P 3306 -u <user> -p
```

The product is an explicit flag. The CLI does not detect it, because the only
signal it could use is the agent URL. airo-go makes up an agent URL for apps
that have no agent (see below), so that signal is not reliable.

## Authentication and authorization

| Layer | Check | Failure |
| --- | --- | --- |
| airo-go | The OAuth JWT is valid (`/v1/hosting` chain) | `401` (`503` if the signing keys are unavailable) |
| airo-go | The customer has a shopper ID and passes the TLA gate | `403` |
| airo-go | The token has the scope `hosting.database.tunnel:execute` | `403 Insufficient scope` |
| airo-go | Rate limit per app and customer | `429` |
| airo-go | The caller is the app's owner and the owner has a shopper ID | `404 app not found` |
| hosting | The service credential is allowed (`JWTOrCert`: Airo console or CTK cert) | airo-go answers `502` |
| hosting | The app exists and its composition defines an `https` agent URL | `404` |
| airo-builder | Grants `canTunnelDatabase` for the cert2s delegation and the `AiroAppBuilder` product | hosting `422`, which airo-go answers as `403` |
| hosting | The token's `exp` is within 1 hour | `502` |
| agent | The token signature, the `canTunnelDatabase` claim, and a token app that matches the path app | WebSocket `401`/`403` |
| MySQL | The database user's own password, `GRANT`s, and TLS, end to end | a MySQL error |

airo-go uses fixed error texts for hosting's `401` and `403`, because those
mean airo-go's own service credential failed. Hosting's other error bodies are
fixed texts and are passed through, so the CLI sees messages such as "App does
not have an agent URL".

Compared with Node.js Hosting:

- **Who validates OAuth.** For Node.js, HWA validates the OAuth token. For
  WordPress, airo-go's shared `/v1/hosting` OAuth chain validates it with the
  same rules. airo-builder receives the OAuth token only as `X-OAuth-Token`,
  next to the cert2s delegation that it bases its grant on.
- **Ownership.** For Node.js, the app is looked up for the authenticated
  customer. For WordPress, airo-go resolves the app's owner through its
  subscription and compares that owner with the caller. Collaborators are
  refused.
- **Required scope.** airo-go checks only the tunnel scope. The CLI still
  requests deploy-execute as well, so both products use the same credential.

## Blocker: no agent on Managed WordPress

The flow above depends on the app having an agent. Managed WordPress apps do
not have one:

- The `managed-wordpress` composition
  (`backends/hosting/templates/compositions/managed-wordpress/v1`) defines only
  the `publish` and `staging` variants. Its only URL patterns are
  `{appId}.<cell>.myftpupload.com` and `{appId}-staging.<cell>.myftpupload.com`.
  `requiredApps` is empty, and the only on-demand jobs are `ssh` and `pma`. No
  job template contains an agent task.
- Only the `ai-builder` and `paas-nodejs` compositions define an agent task and
  an `agent` URL pattern.
- airo-go's `EnhanceApp` makes up agent and preview URLs whenever hosting
  returns none, so the Airo app API shows an agent URL for a WordPress app
  anyway. For example, on test, `sd4prorehl` shows an agent URL that has no DNS
  record. Do not use that URL as proof that an agent exists.

Because hosting builds the agent URL from the composition and not from
airo-go's made-up URL, the mint **fails closed** today. The CLI gets
`404 App does not have an agent URL`, and no token is minted.

To remove the blocker, the Managed WordPress job must run something that can
reach the site's database and accept the tunnel WebSocket. The options are:

1. **An agent task or sidecar in the `managed-wordpress` job**, with an `agent`
   URL pattern and routing for it. This runs for the full life of the site.
2. **A tunnel-only on-demand job**, like the existing `ssh` and `pma` jobs. It
   starts when a tunnel is requested and gets its own URL. It uses resources
   only when a tunnel is in use, but the mint must then wait for the job to be
   ready.

Either option is a hosting template change, and it decides which URL hosting
returns in step 7.

## Cross-team dependencies

| Owner | Change | State |
| --- | --- | --- |
| CLI | `--product wordpress` and `get_airo_database_tunnel_token` | Implemented. The checks, clippy, 951 tests, and the module-size check pass. |
| airo-go | `POST /v1/hosting/apps/:appId/database-tunnel/agent-token` on the shared `/v1/hosting` OAuth chain (BACK-3991): scope check, rate limit, owner check, and a proxy to hosting | Implemented. The route is registered only when `oauthIssuer` and `oauthJwksUrl` are configured. The build and the handler tests must run with the Artifactory `GOPROXY`. |
| hosting | `POST /hosting/v1/apps/:id/database-tunnel/agent-token`: resolves the agent URL, cert2s delegation, airo-builder mint, and a token lifetime check | Implemented. The service is wired only when the airo-builder URL and the cert client are configured. |
| hosting | An agent, or an on-demand tunnel job, for `managed-wordpress` | **Not started. This is the blocker.** |
| Public routing (owner to be identified) | Confirm the public URL for airo-go's `/v1/hosting/apps/:appId/database-tunnel/agent-token` and that it forwards an OAuth `Authorization: Bearer` header unchanged | Not confirmed. The CLI path `/v1/airo/hosting/...` is an assumption and changes if the confirmed URL differs. |
| airo-builder (AAB) | `resolveDatabaseTunnelGrant` grants for cert2s with `AiroAppBuilder`, and the agent's `PaaSNodeJS` product gate is widened only together with the `canTunnelDatabase` check | For the owner of the AAB OAuth/JWT work. It is not part of this change. |

## Deferred work

- **Collaborators.** Only the owner can use this path. Supporting collaborators
  would need the OAuth customer mapped to collaborator capabilities.
- **Re-mint before expiry.** This is the same as for Node.js: the token lives
  one hour and the CLI does not renew it.
- **Detecting the product.** Once only apps with a real agent return an agent
  URL, the CLI could choose the mint itself and `--product` could become
  optional.

## Testing

- **CLI.**
  - `rust/src/db/tunnel.rs`: `product_defaults_to_nodejs_and_accepts_wordpress`
    checks that the default is `nodejs`, that `wordpress` is accepted, and that
    unknown values are rejected.
  - `rust/src/hosting/client_tests.rs`: `get_airo_database_tunnel_token_posts_to_airo_path`
    checks the request path, the Bearer header, and the parsed response.
- **airo-go.**
  - The shared OAuth chain has its own tests (`auth/oauth_access_token_validator_test.go`,
    `middleware/oauth_auth_test.go`, `handlers/hosting_router_test.go`).
  - `handlers/database_tunnel_proxy_handler_test.go` covers the owner check,
    that every miss returns the same 404, and that hosting's `422` becomes
    `403`. Router-level tests run through the real `/v1/hosting` chain. They
    check that the route is registered only when that chain is configured,
    that `sso-jwt` and invalid Bearer tokens get `401` before the owner lookup,
    that a token without the tunnel scope gets `403`, that the validated token
    is forwarded to hosting, and that the sixth request in a burst gets `429`.
- **hosting.** `database_tunnel_service_test.go`, `database_tunnel_handler_test.go`,
  and `sharetokenapi/agent_token_test.go` cover agent URL selection, the refusal
  of non-https agent URLs, the error mapping, and the token lifetime checks.
- **End to end.** Not possible until the blocker is removed and the public
  route to airo-go is confirmed.
