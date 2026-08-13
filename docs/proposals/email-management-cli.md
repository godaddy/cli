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

When the response is ineligible, or an eligible account carries outstanding
`requirements`, attach a `next_actions` entry pointing at `email create` with
the relevant `--account-id`/`--consent` pre-filled from the response.

### `gddy email create`

| | |
|---|---|
| Flags | `--email <email>` (required), `--account-id`, `--first-name`, `--last-name`, repeatable `--consent <agreementType>` |
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
| Flags | `--status`, `--page`, `--page-size`, `--fields` |
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

### 3. List pagination UX: native `--page`/`--page-size` vs generic `--limit`/`--offset`

The server paginates natively (`page`/`pageSize`, capped at 100, with
HATEOAS `links`), which doesn't fit cli-engine's `PaginationConfig` model —
that model expects the handler to return the *complete* list and lets the
engine slice it via generic `--limit`/`--offset`, the way `domain list` does.

**Recommendation: native `--page`/`--page-size` flags, forwarded 1:1 to the
API's query params**, rather than adopting `--limit`/`--offset` via
`PaginationConfig`. Translating page-based server pagination into offset math
would be lossy and would hide the server's own `links`. This is a deliberate
deviation from the `domain list` precedent, called out explicitly here so
it's a conscious choice rather than an inconsistency someone trips over later.

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
