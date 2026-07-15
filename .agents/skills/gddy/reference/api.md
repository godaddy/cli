# GoDaddy Domains API (raw HTTP fallback)

Use this when the `gddy` CLI cannot be installed or isn't a good fit for the task (CI pipelines, non-shell languages, etc.). When `gddy` is available, prefer it — it handles auth, quote-token bookkeeping, and idempotency for you. To install `gddy` itself, see the Setup section in SKILL.md.

Base URL: `https://api.godaddy.com`

Auth header: `Authorization: Bearer $GDDY_PAT` (PAT generated from the Personal Access Token page under `https://developer.godaddy.com/docs/api-users/auth`). Legacy `sso-key key:secret` credentials still work for v1/v2 endpoints but not v3, and are deprecated — prefer a PAT for anything new.

## Endpoint reference

GoDaddy publishes machine-readable OpenAPI specs — fetch and search these directly instead of relying on hardcoded examples here, so this file doesn't need updating as the API grows:

- Search, availability, registration (quote → register), DNS records: `https://developer.godaddy.com/openapi/domains-v3.json`
- Domain listing, bulk availability, contacts, transfers, legacy purchase: `https://developer.godaddy.com/openapi/domains-v1.json`

## Gotchas that won't change even as the API grows

- **Domain registration is two calls, never one** — mirrors the CLI's `gddy domain quote` / `gddy domain purchase` split (run `gddy guide domain-purchase` for the full walkthrough). Lock a quote first — valid ~10 minutes — then register against it with an `Idempotency-Key` header (required, not optional). Reuse the same key when retrying the same logical attempt; a new key on retry can register and charge for the domain twice.
- **MCP server** (`https://api.godaddy.com/v1/domains/mcp`, streamable-http) is read-only and needs no authentication — public domain search/availability only, never registration, DNS, or anything account-specific. Full docs: `https://developer.godaddy.com/docs/api-users/mcp`.
