# Supporting Key User Flows — CLI & Developer Portal Updates

**Date:** 2026-08-20
**Status:** Draft
**Depends on:** [API Access Control Comparison](./API_ACCESS_CONTROL_COMPARISON.md)

---

## Target Flows

This document walks through four concrete user flows, identifies what breaks or is painful
in the current CLI/Developer Portal, and proposes specific changes to support each.

| # | Flow | Persona |
|---|------|---------|
| F1 | First-run CLI purchase + deploy | New developer, interactive |
| F2 | DNS records from CI | CI/CD pipeline (GitHub Actions) |
| F3 | Airo app calling APIs on its own behalf | Server-to-server, app-scoped |
| F4 | CI-triggered deploy on merge | CI/CD pipeline, narrowly scoped |

---

## F1: First-run purchase + deploy

> A C1, using the CLI for the first time with no existing setup, wants to buy a domain and
> deploy a Node.js app to hosting in one continuous session.

### What happens today

```
1. gddy auth login
   → Opens browser, grants: domains.domain:read, apps.app-registry:read, offline_access
   → Token stored in keychain

2. gddy domain available example.com        ✅ works (domains.domain:read)

3. gddy domain quote example.com            ✅ works (domains.domain:read)

4. gddy domain purchase --quote-token <t> --agree --confirm
   → ❌ SCOPE STEP-UP: needs domains.domain:create
   → Opens browser AGAIN for re-consent
   → User must approve wider scopes

5. gddy platform app init my-app
   → ❌ SCOPE STEP-UP: needs apps.app-registry:write
   → Opens browser AGAIN (third time)
   → Creates app, writes godaddy.toml + .env

6. gddy hosting nodejs app create --name my-app
   → ❌ SCOPE STEP-UP: needs hosting.paas.apps:create
   → Opens browser AGAIN (fourth time!)

7. gddy hosting nodejs source upload
   → ❌ SCOPE STEP-UP: needs hosting.paas.code:write
   → Opens browser AGAIN (fifth time!)

8. gddy hosting nodejs deployment publish
   → ❌ SCOPE STEP-UP: needs hosting.paas.deploy:execute
   → Opens browser AGAIN (sixth time!)
```

**Result: 6 browser opens in one session.** Each scope step-up re-opens the browser for
OAuth re-consent. This is technically correct (least-privilege) but terrible UX for a
first-time user doing a common multi-product flow.

### What needs to change

#### Change 1: `gddy auth login --workflow purchase-and-deploy`

Add **workflow presets** that pre-request the scopes needed for common multi-step flows:

```bash
# Request all scopes needed for the purchase+deploy flow upfront
gddy auth login --workflow purchase-and-deploy
```

Workflow presets (defined in CLI, not server-side):

| Workflow | Scopes requested |
|----------|------------------|
| `purchase-and-deploy` | `domains.domain:read`, `domains.domain:create`, `domains.dns:update`, `apps.app-registry:read`, `apps.app-registry:write`, `hosting.paas.apps:create`, `hosting.paas.code:write`, `hosting.paas.deploy:execute`, `offline_access` |
| `domain-management` | `domains.domain:read`, `domains.domain:create`, `domains.dns:update`, `domains.nameserver:update`, `offline_access` |
| `hosting` | `hosting.paas.*` (all hosting scopes), `apps.app-registry:read`, `apps.app-registry:write`, `offline_access` |
| `all` | Every scope in `scopes::ALL` + `offline_access` |

**Implementation:** Pure CLI-side change. Add a `--workflow` flag to `auth login` that maps
preset names to scope lists. No server or cli-engine changes needed.

```rust
// In auth login handler or a new workflows module
fn workflow_scopes(name: &str) -> Option<&[&str]> {
    match name {
        "purchase-and-deploy" => Some(&[
            scopes::DOMAINS_READ, scopes::DOMAINS_CREATE,
            scopes::DOMAINS_DNS_UPDATE, scopes::APP_REGISTRY_READ,
            scopes::APP_REGISTRY_WRITE,
            scopes::HOSTING_APPS_CREATE, scopes::HOSTING_CODE_WRITE,
            scopes::HOSTING_DEPLOY_EXECUTE, scopes::OFFLINE_ACCESS,
        ]),
        "domain-management" => Some(&[...]),
        "hosting" => Some(&[...]),
        "all" => Some(scopes::ALL),
        _ => None,
    }
}
```

#### Change 2: Interactive wizard with bundled auth

The [Interactive Domain Wizard](./INTERACTIVE_DOMAIN_WIZARD.md) design doc already proposes
`gddy domain register --interactive`. Extend this to include hosting setup:

```bash
gddy quickstart
# or
gddy domain register --interactive --with-hosting
```

The wizard knows all the scopes it will need upfront and requests them in a single
`auth login` before starting the interactive flow. One browser open, one consent prompt.

**Implementation:**

```
gddy quickstart
├─ 1. Detect auth status: if not logged in or missing scopes, run auth login
│     with union of all scopes the wizard might need
├─ 2. Domain discovery (suggest/available)
├─ 3. Quote review
├─ 4. Purchase (if user confirms)
├─ 5. "Would you like to deploy an app on this domain?" (prompt)
├─ 6. App init + hosting create + source upload + deploy
└─ 7. DNS: A record pointing domain to hosting IP (auto-configured)
```

#### Change 3: Scope batching in cli-engine (longer-term)

cli-engine's step-up flow could batch pending scope requests. Instead of re-authing on each
new scope, accumulate scopes from the next N commands and request them all at once:

```
middleware.run(command1)  →  needs scope A, not yet granted → QUEUE
middleware.run(command2)  →  needs scope B, not yet granted → QUEUE
// Before first handler execution: request A+B together → ONE browser open
```

This is a cli-engine architectural change and more complex. The workflow preset approach is
the pragmatic short-term solution.

### Summary for F1

| Change | Layer | Effort | Impact |
|--------|-------|--------|--------|
| `--workflow` login presets | CLI | Small | Eliminates multi-step-up for known flows |
| `gddy quickstart` wizard | CLI | Medium | Great first-run UX |
| Scope batching in engine | cli-engine | Large | Eliminates step-up pain for all flows |

---

## F2: DNS records from CI

> A C1 wants DNS records for their domain to be created automatically by a GitHub Actions
> workflow when a pull request is opened or merged.

### What happens today

```yaml
# .github/workflows/dns.yml
env:
  GDDY_PAT: ${{ secrets.GDDY_PAT }}

steps:
  - run: gddy dns add example.com --type A --name @ --data 1.2.3.4
```

**Problems:**

1. **PAT is overly broad** — `GDDY_PAT` with `domains.dns:update` scope can modify DNS for
   *every domain* the user owns, not just `example.com`. If the secret leaks, all domains are
   exposed.

2. **PAT has no expiry** — the secret in GitHub Actions lives forever unless manually rotated.

3. **No audit trail tying the change to the PR** — the DNS update is indistinguishable from
   any other API call with that PAT.

4. **PAT can't be created from the CI setup script** — user must visit the Developer Portal,
   create a PAT, copy it, paste it into GitHub Secrets. This is a multi-step manual process.

### What needs to change

#### Change 4: Resource-scoped PATs (API gateway + Developer Portal)

Allow PATs to be scoped to specific resources:

```
Developer Portal → Create PAT:
  Name: "CI DNS for example.com"
  Environment: prod
  Scopes: domains.dns:update
  Resource filter: domain:example.com    ← NEW
  Expires: 90 days                       ← NEW
```

The gateway validates that the PAT's resource filter matches the API call's target resource.
A call to `PATCH /v1/domains/other-domain.com/records` with this PAT returns 403.

**Implementation requirements:**

| Component | Change |
|-----------|--------|
| Developer Portal | Add resource filter and expiry fields to PAT creation UI |
| API Gateway | Validate resource filter on incoming PAT-authenticated requests |
| CLI | Surface resource filter and expiry in `gddy pat list` |
| PAT format | Encode resource scope in the token claims or a server-side lookup |

#### Change 5: `gddy pat create` from the CLI (API + CLI)

Allow creating PATs from the CLI itself, removing the Portal round-trip:

```bash
# Create a PAT scoped to DNS for one domain, with 90-day expiry
gddy pat create "CI DNS" \
  --scope domains.dns:update \
  --resource "domain:example.com" \
  --expires 90d

# Output:
# {
#   "token": "gd_pat_abc123...",
#   "name": "CI DNS",
#   "scopes": ["domains.dns:update"],
#   "resource": "domain:example.com",
#   "expires_at": "2026-11-18T00:00:00Z"
# }
```

This requires a server-side API endpoint for PAT creation (doesn't exist today). The CLI
sends the authenticated user's OAuth token to create a PAT on their behalf.

#### Change 6: GitHub Actions integration helper

```bash
# One-liner to set up a GitHub Actions secret with a scoped PAT
gddy ci setup github-actions \
  --repo myorg/my-repo \
  --scope domains.dns:update \
  --resource "domain:example.com" \
  --expires 90d \
  --secret-name GDDY_PAT

# Creates scoped PAT → sets it as GitHub secret → outputs workflow snippet
```

This uses the GitHub API (via `gh` or direct) to write the secret, combining PAT creation
with CI setup in one step.

### Summary for F2

| Change | Layer | Effort | Impact |
|--------|-------|--------|--------|
| Resource-scoped PATs | Gateway + Portal | Large | Limits blast radius for leaked CI credentials |
| `gddy pat create` from CLI | API + CLI | Medium | Eliminates Portal round-trip |
| GitHub Actions setup helper | CLI | Small | Streamlines CI onboarding |

---

## F3: Airo app calling APIs on its own behalf

> A C1's Airo app needs to call GoDaddy's email marketing, CRM, and email sending APIs, and
> the C1 wants assurance that this access can only be used by that one app, not by anything
> else.

### What happens today

When a developer creates an app via `gddy platform app init`, the system generates:
- `client_id` (UUID) in `godaddy.toml`
- `client_secret` in `.env`
- `authorization_scopes` in `godaddy.toml`

These are **app-level OAuth client credentials** — separate from the developer's personal
PAT or OAuth session. The app uses these to authenticate as itself, not as a user.

**Problems:**

1. **No client_credentials grant flow in the CLI** — cli-engine has a `ClientCredentialsInjector`
   in its transport layer, but `gddy` doesn't expose it as a user-facing auth flow. There's no
   `gddy auth login --app` to test app-level API calls.

2. **App scopes are declared, not enforced at creation** — `authorization_scopes` in
   `godaddy.toml` is a declaration. The actual enforcement depends on the app registry and
   gateway honoring those scopes when the app authenticates.

3. **No CLI command to test app-as-itself** — a developer can't easily verify "what can my
   app access?" from the CLI. They have to deploy the app and test in production.

4. **No scope isolation between apps** — if a developer has two apps (Airo + a separate CRM
   integration), both created under the same account, there's no guarantee that app A's
   `client_secret` can't be used with app B's scopes.

### What needs to change

#### Change 7: `gddy app auth` — test app-level authentication

```bash
# Authenticate as the app (client_credentials flow), not as the developer
gddy app auth login

# Uses godaddy.toml client_id + .env client_secret
# Returns an app-scoped access token
# Stores it separately from the developer's personal credential
```

```bash
# Test an API call as the app
gddy app api call GET /v1/email/marketing/campaigns --as-app

# Shows what the app can actually access with its own credentials
```

**Implementation:** Add an `--as-app` flag or `app auth` subcommand that uses
cli-engine's existing `ClientCredentialsInjector` to authenticate with the project's
`client_id`/`client_secret`, then makes API calls with that token instead of the
developer's personal token.

#### Change 8: App credential isolation verification

```bash
gddy app auth verify
# Output:
# App: my-airo-app (client_id: 550e8400-...)
# Authorized scopes: email.marketing:read, email.marketing:write, crm:read
# Accessible APIs:
#   ✓ GET /v1/email/marketing/campaigns
#   ✓ POST /v1/email/send
#   ✗ GET /v1/domains (NOT authorized)
#   ✗ POST /v1/domains/purchase (NOT authorized)
# Isolation: This credential ONLY works for this app's client_id
```

This calls a token introspection or scope-check endpoint to show exactly what the app
can and cannot do.

#### Change 9: Scope guardrails at app creation

When `gddy platform app init` asks for `authorization_scopes`, validate them against
the available scope registry and warn if the requested scopes seem broader than needed:

```
What scopes does your app need?
  [x] email.marketing:read
  [x] email.marketing:write
  [x] crm.contact:read
  [ ] domains.domain:read (not selected — your app doesn't need domain access)

⚠ Principle of least privilege: only request scopes your app actually uses.
  You can add more scopes later with `gddy platform app update --add-scope`.
```

### Summary for F3

| Change | Layer | Effort | Impact |
|--------|-------|--------|--------|
| `gddy app auth login` (client_credentials) | CLI | Medium | Developers can test as their app |
| `gddy app auth verify` | CLI + API | Medium | Confirms isolation, builds trust |
| Scope guardrails at init | CLI | Small | Prevents over-permissioning |

---

## F4: CI-triggered deploy on merge

> A C1 wants merging to main to automatically deploy their Node.js app to hosting via
> GitHub Actions, without that pipeline being able to do anything beyond deploying that
> one app.

### What happens today

```yaml
# .github/workflows/deploy.yml
on:
  push:
    branches: [main]

env:
  GDDY_PAT: ${{ secrets.GDDY_PAT }}

jobs:
  deploy:
    steps:
      - uses: actions/checkout@v4
      - run: gddy hosting nodejs source upload
      - run: gddy hosting nodejs deployment publish
```

**Problems:**

1. **PAT can deploy ANY app** — the PAT with `hosting.paas.deploy:execute` can deploy to
   any hosting app the user owns, not just the one in this repo. If the CI secret leaks, an
   attacker can deploy arbitrary code to any of the user's apps.

2. **PAT can do more than deploy** — with `hosting.paas.code:write` + `hosting.paas.deploy:execute`,
   the PAT might also be able to read secrets, delete apps, or modify configurations (depending
   on how scopes are grouped).

3. **No connection between the git repo and the app** — the deployment target is implicit (from
   `godaddy.toml` in the repo). There's no server-side enforcement that "this PAT can only deploy
   app X."

4. **No short-lived credentials** — the PAT in GitHub Secrets is long-lived. A compromised
   secret stays valid until someone notices and manually revokes it.

### What needs to change

#### Change 10: App-scoped deploy tokens

A new token type that can **only** deploy a specific app:

```bash
# Create a deploy-only token for this specific app
gddy hosting deploy-token create \
  --app my-app \
  --expires 365d

# Output:
# {
#   "token": "gd_deploy_abc123...",
#   "app": "my-app",
#   "app_id": "550e8400-...",
#   "permissions": ["source:upload", "deployment:publish"],
#   "expires_at": "2027-08-20T00:00:00Z"
# }
```

This token:
- Can **only** upload source and publish deployments for `my-app`
- Cannot create/delete/modify the app itself
- Cannot access other apps, domains, DNS, or any other resource
- Has a maximum lifetime (e.g., 1 year, enforced server-side)

**Implementation:** This is a specialized PAT with a built-in resource filter for one hosting
app. The gateway validates that the token's `app_id` matches the target of every API call.

#### Change 11: GitHub Actions deploy workflow template

```bash
# One-command CI setup
gddy hosting deploy-token setup github-actions \
  --app my-app \
  --repo myorg/my-repo

# Creates deploy token → sets GitHub secret → generates workflow file
```

Generates `.github/workflows/gddy-deploy.yml`:

```yaml
name: Deploy to GoDaddy Hosting
on:
  push:
    branches: [main]

jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install GoDaddy CLI
        run: curl -fsSL https://cli.godaddy.com/install.sh | sh

      - name: Deploy
        env:
          GDDY_PAT: ${{ secrets.GDDY_DEPLOY_TOKEN }}
        run: |
          gddy hosting nodejs source upload
          gddy hosting nodejs deployment publish --follow
```

#### Change 12: OIDC federation for GitHub Actions (long-term)

Instead of storing a long-lived PAT as a GitHub Secret, use GitHub's OIDC identity provider
to exchange a short-lived GitHub Actions JWT for a short-lived GoDaddy deploy token:

```yaml
permissions:
  id-token: write  # Required for OIDC

steps:
  - name: Get GoDaddy deploy token
    uses: godaddy/cli-action@v1
    with:
      app: my-app
      # No stored secret needed — uses GitHub OIDC

  - name: Deploy
    run: gddy hosting nodejs deployment publish
```

**How it works:**

1. GitHub Actions mints a short-lived JWT identifying the repo, branch, and workflow.
2. GoDaddy's token endpoint validates the JWT against the app's registered GitHub repo.
3. GoDaddy returns a short-lived deploy token (e.g., 15-minute TTL) scoped to that one app.
4. No long-lived secrets stored anywhere.

This is the gold standard (AWS, GCP, Azure, Hashicorp all support OIDC federation from
GitHub Actions). It requires a new GoDaddy token exchange endpoint.

### Summary for F4

| Change | Layer | Effort | Impact |
|--------|-------|--------|--------|
| App-scoped deploy tokens | Gateway + API | Medium | Blast radius = one app |
| GitHub Actions setup helper | CLI | Small | Streamlines CI onboarding |
| OIDC federation | Gateway + CLI Action | Large | Zero stored secrets, short-lived credentials |

---

## Cross-cutting: What Each Flow Needs

```
                             F1: First-run  F2: DNS CI  F3: App auth  F4: Deploy CI
                             ─────────────  ──────────  ────────────  ─────────────
Workflow login presets (C1)       ●
Quickstart wizard (C2)           ●
Resource-scoped PATs (C4)                       ●                         ●
CLI PAT creation (C5)                           ●                         ●
CI setup helpers (C6, C11)                      ●                         ●
App auth/verify (C7, C8)                                     ●
Scope guardrails (C9)                                        ●
Deploy tokens (C10)                                                       ●
OIDC federation (C12)                           ○                         ●

● = required   ○ = nice-to-have
```

---

## Implementation Priority

### Phase 1 — CLI-only changes (no API/gateway work)

These can ship immediately with changes only to the `gddy` Rust codebase:

| # | Change | Effort | Files touched |
|---|--------|--------|---------------|
| C1 | `--workflow` login presets | 1–2 days | `auth.rs`, new `workflows.rs` |
| C9 | Scope guardrails at app init | 1 day | `application/commands/init.rs` |

### Phase 2 — CLI + new API endpoints

These require new server-side API endpoints but no gateway architecture changes:

| # | Change | Effort | Dependencies |
|---|--------|--------|--------------|
| C5 | `gddy pat create` from CLI | 1 week | PAT creation API endpoint |
| C7 | `gddy app auth login` (client_credentials) | 3–5 days | Token endpoint must support client_credentials grant |
| C8 | `gddy app auth verify` | 2–3 days | Token introspection endpoint |
| C2 | `gddy quickstart` wizard | 1–2 weeks | Builds on C1 + wizard framework from [INTERACTIVE_DOMAIN_WIZARD.md](./INTERACTIVE_DOMAIN_WIZARD.md) |

### Phase 3 — Gateway + infrastructure changes

These require API gateway modifications:

| # | Change | Effort | Dependencies |
|---|--------|--------|--------------|
| C4 | Resource-scoped PATs | 2–4 weeks | Gateway resource-level validation |
| C10 | App-scoped deploy tokens | 1–2 weeks | Gateway app-level token validation |
| C6/C11 | CI setup helpers | 1 week | C5 or C10 must exist first |

### Phase 4 — Platform-level changes

| # | Change | Effort | Dependencies |
|---|--------|--------|--------------|
| C12 | OIDC federation for GitHub Actions | 4–6 weeks | OIDC token exchange endpoint, trust policy management |
| C3 | Scope batching in cli-engine | 2–3 weeks | cli-engine architectural change |

---

## How This Maps to the Industry

| Capability | Stripe | Cloudflare | GitHub | Vercel | GoDaddy (proposed) |
|---|---|---|---|---|---|
| Scoped CI tokens | RAK per service | Zone-scoped token | Fine-grained PAT per repo | Project-scoped token | **App-scoped deploy token (C10)** |
| Resource filtering | By permission category | Per zone/resource | Per repository | Per project | **Per domain/app (C4)** |
| CLI token creation | No | `wrangler` API | `gh auth` | `vercel tokens add` | **`gddy pat create` (C5)** |
| App-as-itself auth | API key per service | — | GitHub App installation token | — | **`gddy app auth` (C7)** |
| CI OIDC federation | No | No | No (but GitHub Apps) | No | **OIDC federation (C12)** |
| Workflow presets | No | No | No | No | **`--workflow` presets (C1)** — unique to GoDaddy |

**Notable:** The `--workflow` login preset (C1) and automatic scope step-up combination would
be unique in the industry. No other CLI offers "tell us what you're planning to do and we'll
request the right scopes upfront" combined with "if you need more later, we'll get them
automatically."

---

## Appendix: GitHub Actions Workflow (F2 + F4 combined)

End-state workflow using proposed features:

```yaml
name: Deploy on merge + update DNS
on:
  push:
    branches: [main]

permissions:
  id-token: write  # For OIDC (Phase 4)

jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install GoDaddy CLI
        run: curl -fsSL https://cli.godaddy.com/install.sh | sh

      # Phase 2: scoped PAT in GitHub Secret
      # Phase 4: replace with OIDC (no secret needed)
      - name: Deploy app
        env:
          GDDY_PAT: ${{ secrets.GDDY_DEPLOY_TOKEN }}
        run: |
          gddy hosting nodejs source upload
          gddy hosting nodejs deployment publish --follow

      # Separate scoped token for DNS (least privilege)
      - name: Update DNS
        env:
          GDDY_PAT: ${{ secrets.GDDY_DNS_TOKEN }}
        run: |
          gddy dns add example.com --type A --name @ --data ${{ steps.deploy.outputs.ip }}
```

With resource-scoped tokens:
- `GDDY_DEPLOY_TOKEN` can only deploy `my-app`, nothing else
- `GDDY_DNS_TOKEN` can only modify DNS for `example.com`, nothing else
- Neither token can create/delete apps, purchase domains, or access other resources
