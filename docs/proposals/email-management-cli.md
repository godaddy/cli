# Proposal: `gddy email` — mailbox management commands

Status: draft, seeking CLI-team consensus on the open questions below.

## Motivation

The panel team built a new public-facing API
(`productivity-panel-api/src/server/panel-v3-api/`) that lets customers manage
GoDaddy Email mailboxes over an OAuth bearer token instead of the legacy
shopper-session `panel-api`. We want CLI parity so customers and agents can
create, list, get, and check eligibility for mailboxes directly from `gddy`,
following the same conventions as `domain`/`hosting`. The command surface is
`gddy email` — matching the `email.mailbox:*` OAuth scope family the API is
moving toward — even though the individual resources it manages are called
"mailboxes."

The underlying API doesn't have update/delete yet: only check-eligibility,
create, list, and get are implemented server-side. The scope-authorization
middleware defines `email:update`/`email:delete`/`email:admin` scopes, but no
route currently uses them, so this proposal only covers what's actually
callable today.

## Proposed command tree

```
gddy email list                 # GET /v3/email/mailboxes
gddy email get <mailbox-id>     # GET /v3/email/mailbox/:mailboxId
gddy email create               # POST /v3/email/mailboxes
gddy email check-eligibility    # GET /v3/email/check-eligibility
```

## Per-command spec

### `gddy email check-eligibility`

| | |
|---|---|
| Flags | `--email <email>` (required) |
| Tier | `Read` |
| Scopes | `EMAIL_READ` (`email.mailbox:read`) |

Example output:

```json
{
  "isEligible": false,
  "ineligibleReasons": ["NO_ELIGIBLE_ACCOUNT"],
  "eligibleAccounts": [
    {
      "accountId": "acct-123",
      "requirements": [{ "agreementType": "EMAIL_TOS", "url": "https://..." }]
    }
  ]
}
```

An `accountId` here identifies an existing GoDaddy Email/productivity account
the customer already holds under panel-v3 — it is **not** a shopper/customer
ID, and it has nothing to do with domain or hosting "accounts." See
`gddy guide email-mailboxes` for how accounts, eligibility, and consent fit
together.

When the response is ineligible, or an eligible account carries outstanding
`requirements`, attach a `next_actions` entry pointing at `email create` with
the relevant `--account-id`/`--consent` pre-filled from the response.

### `gddy email create`

| | |
|---|---|
| Flags | `--email <email>` (required), `--account-id` (an existing eligible account's ID, from `check-eligibility`'s `eligibleAccounts[].accountId` — see `gddy guide email-mailboxes`), `--first-name`, `--last-name`, repeatable `--consent <agreementType>` |
| Tier | `Mutate` (`.mutates(true)`) |
| Scopes | `EMAIL_CREATE` (`email.mailbox:create`) |

Example output:

```json
{
  "mailboxId": "mbx-456",
  "email": "someone@example.com",
  "status": "PROVISIONING"
}
```

On a `400`/`422` business-rule failure (missing agreements, no eligible
account), surface a `fix` hint pointing at
`gddy email check-eligibility --email <email>` instead of a generic HTTP
error.

### `gddy email get <mailbox-id>`

| | |
|---|---|
| Args | positional `mailbox-id` |
| Tier | `Read` |
| Scopes | `EMAIL_READ` |

Example output:

```json
{ "mailboxId": "mbx-456", "email": "someone@example.com", "status": "ACTIVE" }
```

### `gddy email list`

| | |
|---|---|
| Flags | `--status`, `--fields`, `--limit`, `--offset` |
| Tier | `Read` |
| Scopes | `EMAIL_READ` |

Example output:

```json
[
  { "mailboxId": "mbx-456", "email": "someone@example.com", "status": "ACTIVE" }
]
```

`get`/`list`/`create` all attach a `next_actions` entry toward `email get
<mailbox-id>` where a mailbox ID is available in the response.

## Open questions for the CLI team

### 1. Feature stage: `Beta` vs `Experimental`

`Stage::Beta` matches `hosting`'s precedent (a real spec, real tests, just
needs field usage before graduating to GA). `Stage::Experimental` matches
`platform`'s precedent (still early, likely to reshape).

**Recommendation: `Beta`.** The API surface is small and stable relative to
what it does support; the open items below are about coordination, not about
the shape of the API changing further.

### 2. Update/delete: omit or stub?

The API doesn't implement `email:update`/`email:delete` routes yet, even
though the scope middleware reserves the scope names. Options: omit those
commands entirely until the API ships them, or scaffold stub commands now
that return a clear "not yet supported by the API" error.

**Recommendation: omit entirely.** Stub commands would need their own dead
scopes (tripping the `scope_registry_non_default_entries_are_wired_to_a_command`
test, or requiring a carve-out from it) and can't be meaningfully tested
against a real API. Adding them once the routes exist is a small, low-risk
follow-up PR.

### 3. List pagination UX: generic `--limit`/`--offset` via `PaginationConfig` (decided)

An earlier draft of this doc claimed cli-engine's `PaginationConfig` model
requires a handler to return the *complete* list before the engine can slice
it via `--limit`/`--offset` — that's stale. `PaginationConfig` supports
`default_limit`/`max_limit`, and a handler can read
`ctx.middleware.limit`/`.offset` *before* it makes its request, so it can
drive its own server-side paging instead of always fetching everything.

**Decision: adopt `--limit`/`--offset` via `PaginationConfig`.** `list`'s
handler translates `--limit`/`--offset` into the server's native
`page`/`pageSize` query params, fetching only as many leading pages as needed
to cover `[0, offset + limit)` — worst case, when `offset` isn't
page-aligned, that's two requests instead of one. The engine's pagination
pipeline then slices the accumulated result to the exact `--limit`/`--offset`
window. This keeps `list` consistent with the `domain list`/`dns list`
precedent instead of introducing a second, native pagination style;
`--page`/`--page-size` are dropped in favor of the generic flags.

Forcing `--offset` to be page-aligned (e.g. rejecting a non-aligned offset
instead of silently paying for the extra request) was considered and
deferred as a possible future enhancement — it isn't required for
correctness today.

## Cross-team coordination needed before this ships

These aren't CLI-team decisions, but they block a working end-to-end command
and are worth surfacing here so they're tracked alongside the UX questions:

- **Scope naming.** The CLI will request the forward-looking dotted scopes
  (`email.mailbox:read`/`email.mailbox:create`), but the currently deployed
  API still enforces the older flat names (`email:read`/`email:create`).
  Either the panel team updates enforcement to accept the dotted names before
  the CLI goes live, or the OAuth authorization server needs to be configured
  to grant whichever scopes the deployed API actually checks.
- **Path prefix.** The CLI targets `/v3/email/...` (matching the OpenAPI
  spec's server template), but the deployed Express app currently mounts
  these routes at `/v3` directly (no `/email` segment). Until the panel team
  aligns the deployed routing with the spec (or adds a `/v3/email` alias),
  CLI requests will 404 against the current deployment.

## Non-goals

- `gddy email update` / `gddy email delete` (see open question #2).
- Any admin-scoped mailbox operations (`email:admin`) — no route exists yet.
