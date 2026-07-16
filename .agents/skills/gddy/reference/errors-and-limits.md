# Errors, retries, and rate limits

## Error shape

Every error response is a stable JSON envelope:

```json
{
  "code": "INVALID_DOMAIN",
  "message": "example is not a valid domain name",
  "fields": [
    { "path": "domain", "code": "INVALID_FORMAT", "message": "..." }
  ]
}
```

Always branch on `code`, never on `message` — message text can change between releases and isn't a stable contract. `fields` is present on validation errors (422) and points at which field(s) failed and why.

429 (rate limited) responses add a `retryAfterSec` field and otherwise carry little useful body — read the headers instead (below).

## Retry semantics — depends on the HTTP method, not just "is it safe to retry"

| Operation | Idempotent? | Notes |
|---|---|---|
| Any `GET` | Yes | Always safe to retry. |
| `POST /v1/domains/purchase` (and registration create) | **No** | Retrying without reusing an `Idempotency-Key` (v3) or otherwise confirming state first can double-purchase/double-charge. Check current state (e.g. does the domain already show up under the account) before retrying a failed purchase. |
| `PUT` (e.g. DNS record replace / `gddy dns set`) | Yes | Safe to retry — it fully replaces state each time. |
| `PATCH` (e.g. DNS record add / `gddy dns add`) | **No** | Appends; retrying a failed "add" can create duplicate records. Check current state before retrying. |
| `DELETE` (e.g. `gddy dns delete`) | Yes | Safe to retry — deleting something already gone is a no-op. |

## Rate limits

60 requests/minute per credential, applied regardless of endpoint. Every response — not just 429s — carries these headers, so you can self-throttle before hitting the limit rather than reacting after:

| Header | Meaning |
|---|---|
| `RateLimit-Limit` | Limit that applies to this request |
| `RateLimit-Remaining` | Requests left in the current window |
| `RateLimit-Reset` | Seconds until the window resets |

On a 429, back off for at least `RateLimit-Reset` seconds (or the `retryAfterSec` field), plus some jitter if retrying programmatically in a loop.

To stay under the limit in the first place:
- Prefer bulk endpoints where they exist (e.g. `POST /v1/domains/available` for many domains at once, instead of looping single `check-availability` calls).
- Use cursor pagination (`limit`/`marker`) sequentially rather than fanning out parallel page requests.
- Don't share one credential across many independent callers and assume the limit is per-account — it's per-credential today, but that's an implementation detail, not a guarantee.

If a legitimate use case needs a sustained rate above 60/min, that's a conversation with GoDaddy developer support, not something to work around client-side.

## Common error codes (registration)

These are the `code` values you'll actually see while quoting/registering a domain, with what each means:

| `code` | Meaning | What to do |
|---|---|---|
| `DOMAIN_NOT_AVAILABLE` | Someone else registered the name first (race between check and purchase). | Re-check availability, suggest alternatives. |
| `BILLING_DECLINED` | Payment method on file was charged and declined. | Surface to the user — they need to fix their payment method. |
| `NO_PAYMENT_PROFILE` | No payment method on the account at all. | Direct the user to `gddy payment-methods add` or the account's payment-methods page. |
| `QUOTE_MISMATCH` | The `period` (or other term) at execute time doesn't match what was quoted. | Reuse the exact period from the quote, or fetch a fresh quote. |
| `INVALID_AGREEMENT_KEYS` | `agreementTypes` sent don't match the TLD's `requiredAgreements` from the quote. | Use the exact keys the quote response returned, not guessed/remembered ones. |
| `MISSING_CONTACT` | No registrant contact configured on the account. | Set up contacts (`gddy domain contacts init`, then fill in the file) before quoting. |
| `MISSING_BILLING_PHONE` | Contact phone is missing or malformed. | Verify the contact has a complete, valid phone number. |

## Common status codes and what to do

- **400** — malformed request (bad JSON, missing required field shape). Fix the request; don't retry as-is.
- **401** — missing/invalid/expired credential. Re-authenticate (`gddy auth login` or refresh the PAT) rather than retry.
- **403** — authenticated but not authorized for this action (e.g. a PAT trying to purchase, which requires OAuth; or no payment method on file). Check `code` to distinguish "wrong credential type" from "account not payment-ready."
- **404** — resource doesn't exist (domain not in the account, unknown operation ID). Don't retry.
- **409** — conflict with current state (e.g. domain already has a pending operation). Check current state before retrying.
- **422** — validation failure; see `fields` for which field(s) and why.
- **429** — rate limited; see above.
- **5xx** — server-side; safe to retry idempotent operations with backoff, not safe to retry non-idempotent ones (see table above) without first checking state.
