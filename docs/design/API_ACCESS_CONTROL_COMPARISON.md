# API & CLI Access Control — Industry Comparison

**Date:** 2026-08-18
**Status:** Analysis

---

## Executive Summary

GoDaddy's CLI and API use **OAuth 2.0 scope-based access control** — a single credential per
environment grants access to all products (domains, hosting, apps, DNS) limited by the scopes
on the token. This is a valid model, but the industry has moved significantly beyond it.

**Key gaps vs. industry leaders:**

1. **No resource-scoped tokens** — GoDaddy PATs grant access to *all* resources of a type (all
   domains, all apps), not specific ones. Stripe, Cloudflare, GitHub, and Vercel all support
   resource-level scoping.
2. **No CLI-managed key creation** — PATs are created in the Developer Portal, not via the CLI.
   Stripe, Cloudflare, and GitHub all support creating scoped tokens from the CLI.
3. **No multi-key management** — one PAT per environment. Stripe recommends separate restricted
   keys per service/integration. Cloudflare supports unlimited scoped tokens.
4. **Opaque PAT scopes** — the CLI doesn't know what scopes a PAT carries. Scope enforcement is
   entirely server-side with no client-side validation or warning.
5. **No key rotation** — no CLI or API support for rotating PATs. Stripe supports rolling keys
   with overlap windows.

---

## GoDaddy Today

### Authentication Model

```
┌─────────────────────────────────────────────────┐
│  gddy CLI                                        │
│                                                   │
│  1. PAT (if configured)                           │
│     GDDY_PAT_PROD > GDDY_PAT > pat.toml          │
│     → Bearer gd_pat_... (gateway exchanges)       │
│                                                   │
│  2. OAuth PKCE (fallback)                         │
│     → Browser login → access + refresh token      │
│     → Scope step-up on first use of new scope     │
│                                                   │
│  Result: ONE bearer token per environment         │
│  Used for: ALL GoDaddy APIs (domains, hosting,    │
│            DNS, apps, GraphQL, API explorer)       │
└─────────────────────────────────────────────────┘
```

### Scope Model

GoDaddy uses `resource:action` OAuth scopes. The CLI declares 16 permission scopes:

| Area | Scopes | Login default? |
|------|--------|---------------|
| App Registry | `apps.app-registry:read`, `:write` | read: yes |
| Domains | `domains.domain:read`, `:create` | read: yes |
| DNS | `domains.dns:update` | No |
| Nameservers | `domains.nameserver:update` | No |
| Hosting (6 scopes) | `hosting.paas.apps:read/create/update/delete`, `code:read/write`, `deploy:execute`, etc. | No |
| Directive | `offline_access` | Yes |

**Step-up flow:** When a command needs a scope the token doesn't carry, cli-engine automatically
triggers a browser re-auth with the wider scope set. The user sees a consent prompt and the token
is upgraded.

### What's Missing

| Capability | GoDaddy | Industry standard |
|---|---|---|
| Resource-level scoping (e.g., "only domain X") | No | Yes (Stripe, Cloudflare, GitHub, Vercel) |
| Multiple keys per environment | No (1 PAT per env) | Yes (Stripe: unlimited RAKs) |
| CLI-managed key creation | No (Portal only) | Yes (Stripe, Cloudflare, GitHub) |
| Key rotation with overlap | No | Yes (Stripe rolling keys) |
| Scope introspection on PATs | No (opaque) | Partial (GitHub shows PAT scopes) |
| Team/org-scoped tokens | No | Yes (Vercel, GitHub) |
| IP/geo restrictions on keys | No | Yes (Cloudflare, Stripe) |
| Key expiry enforcement | No | Yes (GitHub: mandatory expiry) |
| CI-specific tokens | No (same PAT) | Yes (Vercel project-scoped, GitHub fine-grained) |

---

## Industry Comparison

### Stripe — Restricted API Keys (Best-in-class for multi-service)

**Can one key manage all products?** Yes, but Stripe recommends against it.

**Model:** Three key types:
- **Publishable key** (`pk_`) — client-side only, very limited
- **Secret key** (`sk_`) — full access, deprecated for new use
- **Restricted API Key** (`rk_`) — **recommended**, fine-grained permissions

**Fine-grained access:**

```
Restricted Key: "Billing Service Key"
├── Charges:         Read + Write
├── Customers:       Read + Write
├── Invoices:        Read + Write
├── Products:        None
├── Subscriptions:   None
├── Disputes:        None
└── (30+ categories, each: None / Read / Write)
```

| Feature | Detail |
|---|---|
| Per-resource scoping | By permission category (Charges, Customers, etc.), not individual resource |
| Multiple keys | Unlimited; one per service recommended |
| Key creation | Dashboard + API |
| Key rotation | Rolling keys with overlap window (old + new valid simultaneously) |
| IP restrictions | Yes, per key |
| Expiry | No mandatory expiry, but rotation recommended |
| CLI token | Uses `sk_` or `rk_` directly; `stripe login` creates a short-lived pairing token |

**Key insight:** Stripe's model is "many restricted keys, one per integration point." A billing
microservice gets a key with only billing permissions. A reporting service gets read-only. If
one key leaks, blast radius is limited.

### Cloudflare — Resource-scoped tokens (Best-in-class for resource isolation)

**Can one key manage all products?** Yes (Global API Key), but strongly discouraged.

**Model:** Three key types:
- **Global API Key** — legacy, full access to everything (email + key auth)
- **API Token** — recommended, fine-grained, policy-based
- **Origin CA Key** — special-purpose for origin certificates

**Fine-grained access (API Tokens):**

```
Token: "DNS Editor for example.com"
├── Permission Group: DNS Write
├── Resource Scope: com.cloudflare.api.account.zone.{zone_id}
├── Effect: Allow
├── IP Filter: 10.0.0.0/8 only
└── Expiry: 2026-12-31
```

| Feature | Detail |
|---|---|
| Per-resource scoping | **Yes** — per zone, per account, per resource (e.g., specific Load Balancer) |
| Multiple keys | Unlimited |
| Key creation | Dashboard + API + Terraform + CLI (`wrangler`) |
| Key rotation | Manual; tokens can be rolled |
| IP restrictions | Per token, allow + deny lists |
| Expiry | Optional, per token |
| Effect (allow/deny) | Supports deny rules for exceptions (e.g., "all zones except zone X") |

**Key insight:** Cloudflare is the gold standard for resource-level scoping. You can create a
token that can *only* edit DNS for *one specific zone*, and nothing else. This is the most
relevant comparison for GoDaddy because both manage domains and DNS.

**Direct GoDaddy comparison:**

| Scenario | Cloudflare | GoDaddy |
|---|---|---|
| "Edit DNS for example.com only" | API token scoped to zone ID + DNS Write | PAT with `domains.dns:update` = ALL domains |
| "Read-only access to all zones" | API token with Zone Read, all zones | OAuth with `domains.domain:read` = similar |
| "CI key for one project" | Token scoped to one zone + Workers | PAT = all resources in that env |
| "Revoke one key without breaking others" | Delete that token | Only have one PAT per env |

### GitHub — Fine-grained Personal Access Tokens

**Can one key manage all repos?** Classic PATs: yes. Fine-grained PATs: configurable.

**Model:** Two PAT types:
- **Classic PAT** — broad scopes (`repo`, `admin:org`), all repos, no expiry
- **Fine-grained PAT** — per-repo, 50+ granular permissions, mandatory expiry

**Fine-grained access:**

```
Fine-grained PAT: "CI Deploy Key"
├── Repository Access: Only "myorg/frontend"
├── Repository Permissions:
│   ├── Contents:      Read + Write
│   ├── Pull Requests: Read + Write
│   ├── Deployments:   Read + Write
│   └── Metadata:      Read (auto-included)
├── Organization Permissions:
│   └── Members:       None
├── Expiry: 90 days
└── Owner Approval: Required by org policy
```

| Feature | Detail |
|---|---|
| Per-resource scoping | **Yes** — per repository (selected repos only) |
| Multiple keys | Unlimited |
| Key creation | Web UI + API (`POST /user/installations`) |
| Key rotation | Manual; expiry forces rotation |
| Expiry | **Mandatory** (max 1 year, org can enforce shorter) |
| Org approval | Orgs can require admin approval before a fine-grained PAT is active |
| CLI integration | `gh auth token` returns current token; `gh auth login` does OAuth |

**Key insight:** GitHub's migration from classic to fine-grained PATs mirrors the evolution
GoDaddy should consider. Classic PATs (broad scope, no resource targeting) → Fine-grained
PATs (per-repo, per-permission, mandatory expiry). GoDaddy's current `gd_pat_*` is closest
to GitHub's classic PATs.

### Shopify — App-scoped OAuth with optional scope step-up

**Can one key manage all products?** No — each app gets its own OAuth credentials.

**Model:** Per-app OAuth installation:
- **Required scopes** — declared in `shopify.app.toml`, granted at install
- **Optional scopes** — can be requested at runtime, merchant can grant or decline
- **Scope revocation** — merchant can revoke optional scopes without uninstalling

| Feature | Detail |
|---|---|
| Per-resource scoping | Not per-resource, but per-store (each install is scoped to one store) |
| Multiple keys | Each app is a separate OAuth client |
| Scope granularity | ~60 scopes (`read_products`, `write_orders`, etc.) |
| Dynamic scopes | Yes — request optional scopes at runtime via consent modal |
| Scope revocation | Yes — `appRevokeAccessScopes` mutation |

**Key insight:** Shopify's `optional_scopes` + runtime request model is the closest analog to
GoDaddy's scope step-up flow. But Shopify's scoping is per-store-installation, not a single
global credential.

### Vercel — Project-scoped tokens (newest entrant, July 2026)

**Can one key manage all projects?** Yes (Full Account scope), but project-scoped is recommended.

**Model:** Three token scopes:

| Scope | Access |
|---|---|
| Full Account | All personal + all teams |
| Team | One team, all projects |
| Project | **One project only** |

```bash
# Create a project-scoped token via CLI
vercel tokens add "Preview deploy bot" --project prj_abc123
```

| Feature | Detail |
|---|---|
| Per-resource scoping | **Yes** — per project |
| Multiple keys | Unlimited |
| Key creation | Dashboard + CLI + API |
| Expiry | Configurable |
| Blast radius | Project-scoped key can't touch other projects, team, or user resources |

**Key insight:** Vercel's model is the simplest form of resource scoping — just three levels.
But the `--project` flag on token creation is a UX pattern GoDaddy could adopt directly.

### AWS — IAM policies (most complex, most powerful)

**Can one credential manage all services?** Yes (root / `AdministratorAccess`), but this is
the #1 anti-pattern.

**Model:** Identity-based + resource-based policies:

```json
{
  "Effect": "Allow",
  "Action": ["s3:GetObject", "s3:PutObject"],
  "Resource": "arn:aws:s3:::my-bucket/my-prefix/*",
  "Condition": {
    "IpAddress": { "aws:SourceIp": "10.0.0.0/8" }
  }
}
```

| Feature | Detail |
|---|---|
| Per-resource scoping | **Yes** — ARN-level (individual S3 bucket, specific Lambda, etc.) |
| Multiple credentials | Unlimited roles, users, access keys |
| Temporary credentials | STS `assume-role` → short-lived session tokens (recommended) |
| CLI profiles | Named profiles in `~/.aws/config` for different roles |
| Policy generation | IAM Access Analyzer auto-generates least-privilege from CloudTrail |
| Boundary policies | Permission boundaries, SCPs, RCPs for org-wide guardrails |
| MFA enforcement | Can require MFA for specific API actions |

**Key insight:** AWS is the extreme end of access control complexity. Most of it isn't relevant
for a developer CLI, but two patterns are: **named profiles for different access levels** and
**temporary credentials** (short-lived tokens derived from longer-lived ones).

---

## Synthesis: Where GoDaddy Stands

### Maturity Model

```
Level 1: Single all-access key
  └─ AWS root key, Cloudflare Global API Key, GitHub classic PAT

Level 2: Scope-based (action permissions, all resources)
  └─ GoDaddy today ◄── YOU ARE HERE
  └─ Shopify (per-store, not per-resource)

Level 3: Scope + resource-scoped (action permissions + specific resources)
  └─ Stripe Restricted Keys (per permission category)
  └─ Vercel project-scoped tokens
  └─ GitHub fine-grained PATs (per repo)

Level 4: Full policy-based (action + resource + condition)
  └─ Cloudflare API Tokens (per zone + IP + expiry + deny rules)
  └─ AWS IAM (ARN-level + conditions + boundaries)
```

### What GoDaddy Does Well

1. **Scope step-up** — cli-engine's automatic OAuth re-consent when a command needs a wider
   scope is excellent UX. Only Shopify has something comparable (dynamic `optional_scopes`).
   No other CLI does this automatically.

2. **Read-only defaults** — only `domains.domain:read`, `apps.app-registry:read`, and
   `offline_access` are requested at login. Mutating scopes are requested on first use.
   This follows least-privilege for the interactive flow.

3. **PAT + OAuth dual path** — supporting both interactive (OAuth PKCE) and non-interactive
   (PAT) auth with clear precedence is good. Most CLIs have this.

4. **Scope registry with tests** — the `scopes.rs` module with compile-time validation that
   every scope is wired to a command and registered on the OAuth client is unique and prevents
   scope drift.

5. **Environment isolation** — separate credentials per environment (`prod` vs `ote`) with
   distinct API endpoints is proper separation.

### Where GoDaddy Falls Short

| Gap | Impact | Industry precedent |
|---|---|---|
| **No resource-level scoping** | A PAT for DNS covers ALL domains. A leaked PAT exposes everything. | Cloudflare per-zone, GitHub per-repo, Vercel per-project |
| **One PAT per environment** | Can't give CI a narrow key and keep a wider one for interactive use | Stripe: unlimited RAKs per integration |
| **No CLI key creation** | Must visit Developer Portal to create/manage PATs | Stripe CLI, Cloudflare `wrangler`, Vercel CLI all create tokens |
| **Opaque PAT scopes** | CLI can't warn when a PAT lacks scopes for a command; user gets a 403 from the API | GitHub shows granted permissions; Stripe RAK creation declares them |
| **No key expiry** | PATs live forever unless manually revoked | GitHub: mandatory expiry on fine-grained PATs |
| **No key rotation** | Replacing a PAT = delete old + create new in Portal, update everywhere | Stripe: overlapping key rotation |
| **No IP/condition restrictions** | Can't restrict where a PAT works from | Cloudflare: IP allow/deny per token |

---

## Recommendations

### Short-term (within existing architecture)

1. **`gddy pat create` via API** — allow creating PATs from the CLI if the Developer Portal
   API supports it. This removes the Portal round-trip for CI setup.

2. **PAT scope introspection** — when a PAT is used and a command fails with 403, detect
   the likely missing scope and suggest: `"This command requires domains.dns:update. Your PAT
   may not have this scope. Create a new PAT with this scope in the Developer Portal."`

3. **Multiple PATs per environment** — change `pat.toml` from `BTreeMap<String, PatEntry>` to
   support named PATs within an environment (e.g., `gddy pat add --env prod --name "CI deploy"
   --name "local dev"`). Use `--pat-name` flag or `GDDY_PAT_NAME` to select.

4. **PAT expiry warnings** — if PATs carry expiry metadata, surface it in `gddy pat list` and
   warn on upcoming expiry.

### Medium-term (API + gateway changes required)

5. **Resource-scoped PATs** — the highest-impact improvement. Allow creating PATs scoped to
   specific resources (e.g., "only domain example.com" or "only app my-app"). This requires
   API gateway support for resource-level token validation.

6. **CLI-managed token lifecycle** — `gddy token create`, `gddy token rotate`, `gddy token
   list`, `gddy token revoke`. This moves token management into the CLI workflow entirely.

7. **Mandatory expiry** — enforce maximum token lifetime (e.g., 1 year) with rotation warnings.

### Long-term (architectural)

8. **Short-lived CI tokens** — like AWS STS `assume-role`, allow exchanging a long-lived PAT
   for a short-lived, narrowly-scoped session token. The CI pipeline gets a token that lives
   for 1 hour and can only deploy one specific app.

9. **Team/org scoping** — for reseller or enterprise accounts, support tokens scoped to a
   specific team or sub-account, not the entire shopper identity.

---

## Appendix: Feature Matrix

| Feature | GoDaddy | Stripe | Cloudflare | GitHub | Shopify | Vercel | AWS |
|---|---|---|---|---|---|---|---|
| OAuth login | PKCE | Stripe Login | — | Device flow | Per-app install | OAuth | SSO/IdC |
| API keys (long-lived) | PAT | Secret + RAK | Global + Token | Classic PAT | — | Access Token | Access Key |
| Fine-grained keys | No | RAK | API Token | Fine-grained PAT | — | Scoped Token | IAM Policy |
| Action scopes | Yes (16) | Yes (~30 categories) | Yes (100+ groups) | Yes (50+ permissions) | Yes (~60) | Private beta | Yes (per API action) |
| Resource scoping | No | By category | **Per zone/resource** | **Per repository** | Per store | **Per project** | **Per ARN** |
| Multiple keys | 1/env | Unlimited | Unlimited | Unlimited | Per app | Unlimited | Unlimited |
| CLI key creation | No | No | `wrangler` API | `gh auth` | `shopify` | `vercel tokens add` | `aws iam` |
| Key rotation | No | Rolling overlap | Manual | Manual (expiry) | N/A | Manual | Rotate access keys |
| IP restrictions | No | Yes | Yes | No | No | No | IAM condition |
| Expiry | No | No | Optional | **Mandatory** | N/A | Optional | Optional |
| Deny rules | No | No | **Yes** | No | No | No | **Yes** |
| Temp credentials | No | No | No | Installation token | No | No | **STS assume-role** |
| Scope step-up | **Yes (automatic)** | No | No | No | **Yes (optional scopes)** | No | No |

---

## References

- [Stripe Restricted API Keys](https://docs.stripe.com/keys/restricted-api-keys)
- [Cloudflare API Token Policies](https://developers.cloudflare.com/fundamentals/api/how-to/create-via-api/)
- [Cloudflare Resource-Scoped Permissions](https://blog.cloudflare.com/improved-developer-security/)
- [GitHub Fine-grained PATs](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens)
- [Shopify Optional Scopes](https://shopify.dev/docs/apps/build/authentication-authorization/app-installation/manage-access-scopes)
- [Vercel Project-Scoped Tokens](https://vercel.com/changelog/project-scoped-tokens)
- [AWS IAM Policies](https://docs.aws.amazon.com/IAM/latest/UserGuide/access_policies.html)
- GoDaddy CLI `scopes.rs` — scope registry with compile-time validation
- GoDaddy CLI `pat/mod.rs` — PAT management (one per environment)
- cli-engine `auth/pkce.rs` — OAuth PKCE with scope step-up
- cli-engine `middleware.rs` — `Authorizer` trait (unused by gddy)
