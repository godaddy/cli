# GoDaddy Domains API (raw HTTP fallback)

Use this when the `gddy` CLI isn't installed or isn't a good fit for the task (CI pipelines, non-shell languages, etc.). When `gddy` is available, prefer it — it handles auth, quote-token bookkeeping, and idempotency for you.

Base URL: `https://api.godaddy.com`

Auth header: `Authorization: Bearer $GODADDY_PAT` (PAT generated from the Personal Access Token page under `https://developer.godaddy.com/docs/api-users/auth`). Legacy `sso-key key:secret` credentials still work for v1/v2 endpoints but not v3, and are deprecated — prefer a PAT for anything new.

## Search and availability

```bash
curl -s "https://api.godaddy.com/v3/domains/check-availability?domain=example.com&optimizeFor=SPEED" \
  -H "Authorization: Bearer $GODADDY_PAT"
```

`optimizeFor` is `SPEED` (cached, fast) or `ACCURACY` (live registry check, slower). Prices are in cents — divide by 100.

```bash
curl -s "https://api.godaddy.com/v3/domains/suggestions?query=coffee+shop&tlds=com,net&pageSize=25&lengthMin=4&lengthMax=15" \
  -H "Authorization: Bearer $GODADDY_PAT"
```

`sources` can further filter suggestion sources: `EXTENSION, KEYWORD_SPIN, CC_TLD, PREMIUM`.

For checking many domains at once instead of one-by-one, `POST /v1/domains/available` accepts an array of names in one request — much cheaper against the rate limit than looping single checks.

## Registration: quote, then execute

Mirrors the CLI's `gddy domain quote` / `gddy domain purchase` split — registration is always two calls, never one.

1. Lock a quote:
   ```bash
   curl -s -X POST "https://api.godaddy.com/v3/domains/registration-quotes" \
     -H "Authorization: Bearer $GODADDY_PAT" -H "Content-Type: application/json" \
     -d '{"domain": "example.com", "period": 1}'
   ```
   Returns `{quoteToken, expiresAt, requiredAgreements}`. The token is single-use and short-lived.

2. Execute the registration against that token:
   ```bash
   curl -s -X POST "https://api.godaddy.com/v3/domains/registrations" \
     -H "Authorization: Bearer $GODADDY_PAT" -H "Content-Type: application/json" \
     -H "Idempotency-Key: $(uuidgen)" \
     -d '{"quoteToken": "<token>", "domain": "example.com", "period": 1, "consent": {"agreementTypes": [...], "agreedAt": "<iso8601>"}}'
   ```
   The `Idempotency-Key` header is required, not optional — registration is not safely retryable without one. Retrying the same request with a *new* key can register (and charge for) the domain twice; always reuse the same key when retrying the same logical attempt.

3. Poll for completion:
   ```bash
   curl -s "https://api.godaddy.com/v3/domains/registrations/<registrationId>" -H "Authorization: Bearer $GODADDY_PAT"
   # or
   curl -s "https://api.godaddy.com/v3/domains/operations/<operationId>" -H "Authorization: Bearer $GODADDY_PAT"
   ```

## Listing domains (pagination)

`GET /v1/domains` uses cursor pagination via `limit` and `marker` — repeat with `marker` set to the last domain name from the previous page until a page comes back shorter than `limit`:

```bash
curl -s "https://api.godaddy.com/v1/domains?limit=100" -H "Authorization: Bearer $GODADDY_PAT"
curl -s "https://api.godaddy.com/v1/domains?limit=100&marker=last-domain-from-previous-page.com" \
  -H "Authorization: Bearer $GODADDY_PAT"
```

v2 list endpoints (domain forwarding, actions, etc.) return the entire collection in one response and don't take `limit`/`marker` at all. A page walk is safe to interrupt and resume — retry the same `(limit, marker)` pair on failure.

## MCP server (read-only, public data only)

`https://api.godaddy.com/v1/domains/mcp` (streamable-http transport) exposes public domain search and availability tools with no authentication required:

```json
{
  "mcpServers": {
    "godaddy": {
      "url": "https://api.godaddy.com/v1/domains/mcp",
      "transport": "streamable-http"
    }
  }
}
```

It cannot register domains, touch DNS, or do anything account-specific — it's a convenience for pure discovery ("is this domain available?", "suggest names for X"). For anything authenticated (purchase, DNS, account domains), use the CLI or the endpoints above instead.

Full docs: `https://developer.godaddy.com/docs/api-users/mcp` — curl the `llms.mdx` mirror per the guidance in SKILL.md rather than fetching that page directly.
