# Interactive CLI Commands — Design Document

This document proposes interactive wizard-style flows for `gddy` commands beyond the existing `domain register` wizard. Each wizard follows the same architecture pattern: `WizardState` struct + `StepResult` (Continue / Back / Cancel) dispatch loop.

## Background

The `domain register` wizard (shipped in the `feat/domain-register-wizard` branch) established a proven interactive pattern for the GoDaddy CLI:

- **5-step wizard**: discovery → options → contacts → review → execute
- **State accumulator**: a single `WizardState` struct passed `&mut` to every step
- **Navigation**: `StepResult::Continue` / `Back` / `Cancel` with a central dispatch loop
- **Bridge entry**: related commands (`domain available`, `domain suggest`, `domain quote`) can hand off into the wizard mid-flow
- **Non-interactive fallback**: all parameters accepted as flags for CI/agent use

This design extends that pattern to 6 additional command groups where multi-step flows, flag complexity, or cross-command ID handoffs create friction.

## Proposed Interactive Flows

### 1. DNS Record Manager — `gddy dns manage` (HIGH priority)

**Current pain points:**
- Adding records requires knowing exact `--type`, `--name`, `--data` flags
- No preview of existing records before destructive `set` / `delete`
- CNAME conflicts discovered only after a failed API call
- TTL defaults (3600) are invisible to users
- Partial failures on multi-record `add` leave ambiguous state

**Interactive flow (6 steps):**

| Step | Prompt Type | Description |
|------|-------------|-------------|
| Domain Selection | Text + Autocomplete | Enter domain or pick from owned domains |
| Action Choice | Select | Add / Edit / Delete / View records |
| Current Records Display | Table (stderr) | Fetch and show zone records; highlight CNAME conflicts |
| Record Configuration | Select + Validated Text | Pick type, enter name/data with type-specific validation |
| TTL & Options | Select | Common TTL presets (600s, 1h, 6h, 24h, custom) |
| Preview & Confirm | Diff Display + Confirm | Before/after diff of the DNS zone; explicit confirmation |

**Terminal mockup:**

```
$ gddy dns manage

  DNS Record Manager
  ──────────────────

? Domain: example.com

  Current records for example.com:
  ┌──────┬──────────┬─────────────────┬──────┐
  │ Type │ Name     │ Data            │ TTL  │
  ├──────┼──────────┼─────────────────┼──────┤
  │ A    │ @        │ 185.199.108.153 │ 3600 │
  │ A    │ @        │ 185.199.109.153 │ 3600 │
  │ CNAME│ www      │ example.com     │ 3600 │
  │ MX   │ @        │ mx.example.com  │ 3600 │
  │ TXT  │ @        │ v=spf1 inclu... │ 3600 │
  └──────┴──────────┴─────────────────┴──────┘

? What would you like to do?
  ❯ Add new records
    Edit existing records
    Delete records
    Cancel

? Record type:
  ❯ A        IPv4 address
    AAAA     IPv6 address
    CNAME    Alias to another domain
    MX       Mail server
    TXT      Text record (SPF, DKIM, verification)
    SRV      Service locator
    CAA      Certificate authority

? Record name: api
? Record data (IPv4 address): 203.0.113.50

? TTL:
  ❯ 1 hour   (3600)  — default
    10 min    (600)   — fast propagation
    6 hours   (21600)
    24 hours  (86400)
    Custom

  Preview:
  ┌────────┬──────┬──────┬──────────────┬──────┐
  │ Action │ Type │ Name │ Data         │ TTL  │
  ├────────┼──────┼──────┼──────────────┼──────┤
  │ + ADD  │ A    │ api  │ 203.0.113.50 │ 3600 │
  └────────┴──────┴──────┴──────────────┴──────┘

? Apply this change? (Y/n) Yes

  ✓ 1 record created

  Verify: gddy dns list example.com --type A --name api
```

**Reuse:** Leverages existing `--dry-run` on `dns set` / `dns delete` for the preview step. The `dns list` fetch is shared with the current `dns list` command.

---

### 2. DNS Batch Editor — `gddy dns edit <zone>` (MEDIUM priority)

**Current pain points:**
- Editing multiple records requires separate `add` / `set` / `delete` commands
- No way to stage changes and apply as a batch
- MX priority + data must be combined in `--data` (e.g., `10 mx1.example.com`)
- TXT record escaping is error-prone on the command line

**Interactive flow (3 steps):**

| Step | Prompt Type | Description |
|------|-------------|-------------|
| Load Zone | Text + Auto | Enter domain; auto-fetch all records; display as editable table |
| Edit Loop | Select (per record) | Navigate records: Edit / Delete / Skip / Add new / Done |
| Change Summary | Diff Table + Confirm | Unified diff of all pending changes; confirm all at once |

**Terminal mockup:**

```
$ gddy dns edit example.com

  DNS Zone Editor: example.com
  ────────────────────────────

  Loading records...
  ┌───┬──────┬──────┬─────────────────┬──────┐
  │   │ Type │ Name │ Data            │ TTL  │
  ├───┼──────┼──────┼─────────────────┼──────┤
  │   │ A    │ @    │ 185.199.108.153 │ 3600 │
  │   │ A    │ @    │ 185.199.109.153 │ 3600 │
  │ ❯ │ CNAME│ www  │ example.com     │ 3600 │
  │   │ MX   │ @    │ 10 mx.example.. │ 3600 │
  │   │ TXT  │ @    │ v=spf1 inclu... │ 3600 │
  └───┴──────┴──────┴─────────────────┴──────┘

? Action for CNAME www → example.com:
  ❯ Edit this record
    Delete this record
    Skip
    Add new record
    Done editing

  (after editing several records)

  Pending changes (3):
  ┌────────┬──────┬──────┬──────────────────┬──────┐
  │ Action │ Type │ Name │ Data             │ TTL  │
  ├────────┼──────┼──────┼──────────────────┼──────┤
  │ ~ EDIT │ CNAME│ www  │ www.example.com  │ 3600 │
  │ + ADD  │ A    │ api  │ 203.0.113.50     │ 600  │
  │ - DEL  │ TXT  │ @    │ v=spf1 inclu...  │ 3600 │
  └────────┴──────┴──────┴──────────────────┴──────┘

? Apply all changes? (Y/n) Yes

  Applying 3 changes...
  ✓ CNAME www updated
  ✓ A api created
  ✓ TXT @ deleted

  All changes applied successfully.
```

---

### 3. Platform App Bootstrap — `gddy platform app bootstrap` (HIGH priority)

**Current pain points:**
- `app init` requires 5+ flags (`--name`, `--url`, `--proxy-url`, `--scopes`, `--description`)
- After init, users must manually chain 4-6 `add` commands for actions/subscriptions/extensions
- Webhook event types must be copied from `platform webhook events` output
- `release` needs `--application-id` from init output and a `--version` string
- `deploy` needs `--name`; full pipeline is 6+ sequential commands
- `add settings` has 10 flags including icon pairs that must go together

**Interactive flow (7 steps):**

| Step | Prompt Type | Description |
|------|-------------|-------------|
| App Identity | Validated Text | Name (3-255 lowercase/digits/hyphens), description, display label |
| URLs | Validated Text | Application URL and proxy URL; inline public-routable validation |
| Scopes | Multi-select | Browse authorization scopes from catalog; pre-select common defaults |
| Components | Multi-select + Sub-wizards | Choose Actions, Webhooks, Extensions, Settings; each launches sub-wizard |
| Actions Sub-wizard | Text (loop) | Per action: name + URL; loop until done; running list |
| Webhooks Sub-wizard | Multi-select + Text | Fetch events from API; multi-select; enter subscription name + URL |
| Review & Create | Summary + Confirm | Full config summary; create app, write godaddy.toml, create release |

**Terminal mockup:**

```
$ gddy platform app bootstrap

  Platform App Bootstrap
  ──────────────────────

  Step 1 of 5: App Identity
  ─────────────────────────
? Application name: my-tax-app
? Display label (my-tax-app): My Tax Application
? Description: Tax calculation service for GoDaddy stores

  Step 2 of 5: URLs
  ─────────────────
? Application URL: https://my-tax-app.example.com
  ✓ URL is publicly routable
? Proxy URL: https://api.my-tax-app.example.com
  ✓ URL is publicly routable

  Step 3 of 5: Authorization Scopes
  ──────────────────────────────────
? Select scopes (space to toggle, enter to confirm):
  ◉ openid
  ◉ profile
  ◉ orders.order:read
  ◯ orders.order:write
  ◉ catalog.product:read
  ◯ catalog.product:write
  ◯ customers.customer:read

  Step 4 of 5: Components
  ───────────────────────
? What components does your app need?
  ◉ Actions (HTTP endpoints the platform calls)
  ◉ Webhook subscriptions
  ◯ UI Extensions
  ◯ Settings placements

  Adding actions:
? Action name: calculate-tax
? Action URL: https://api.my-tax-app.example.com/tax
  ✓ Added: calculate-tax
? Add another action? (y/N) No

  Adding webhook subscriptions:
? Select events to subscribe to:
  ◉ ORDER_CREATED
  ◉ ORDER_UPDATED
  ◯ ORDER_CANCELLED
  ◯ PRODUCT_CREATED
  ◯ CHECKOUT_COMPLETED
? Subscription name: order-events
? Webhook URL: https://api.my-tax-app.example.com/webhooks
  ✓ Added: order-events (2 events)

  Step 5 of 5: Review
  ───────────────────
  ┌─────────────────────────────────────────────┐
  │ Application Summary                         │
  ├─────────────────────────────────────────────┤
  │ Name:        my-tax-app                     │
  │ Label:       My Tax Application             │
  │ URL:         https://my-tax-app.example.com │
  │ Proxy:       https://api.my-tax-app...      │
  │ Scopes:      openid, profile, +2            │
  │                                             │
  │ Actions:     calculate-tax                  │
  │ Webhooks:    order-events (2 events)        │
  │ Extensions:  none                           │
  └─────────────────────────────────────────────┘

? Create application and write godaddy.toml? (Y/n) Yes

  ✓ Application created (id: app-a1b2c3d4)
  ✓ godaddy.toml written
  ✓ .env written

? Create initial release (0.0.1)? (Y/n) Yes
  ✓ Release created (rel-e5f6g7h8)

  Next steps:
  → gddy platform app deploy --name my-tax-app
  → gddy platform app enable my-tax-app --store-id <id>
```

**Reuse:** Calls the same `create_application` GraphQL mutation, `write_config`, and `write_env_file` as the existing `platform app init`. Webhook events fetched from `platform webhook events` API. Release creation reuses `platform app release` handler logic.

---

### 4. Node.js Hosting Deploy Wizard — `gddy hosting nodejs deploy-wizard` (HIGH priority)

**Current pain points:**
- App creation returns a job ID requiring manual polling with a separate command
- Source upload/git import returns another job ID requiring more polling
- App ID must be copied between create, upload, deploy, status, and logs commands
- `logs` command needs 4 mandatory flags (`--app-id`, `--target`, `--source`, `--since`)
- GitHub flow requires chaining: `github status` → `repos` → `branches` → `source git`
- No unified flow from "I have code" to "it is deployed"

**Interactive flow (6 steps):**

| Step | Prompt Type | Description |
|------|-------------|-------------|
| App Selection | Select + Create | List existing apps or create new (name + datacenter) |
| Source Method | Select | Upload zip, Import from GitHub, or use existing source |
| GitHub Flow | Chained Selects | Check connection → list repos (searchable) → select branch |
| Upload Flow | Text (path) | Enter directory path; validate package.json; show file count/size |
| Deploy & Poll | Spinner + Progress | Submit source; auto-poll job; show real-time build progress |
| Publish Decision | Select | Preview only / Publish to production / View logs first |

**Terminal mockup:**

```
$ gddy hosting nodejs deploy-wizard

  Node.js Deploy Wizard
  ─────────────────────

? Select an application:
  ❯ my-node-app     (id: abc-123)  active
    api-service      (id: def-456)  active
    ── Create new application ──

? Deploy source:
  ❯ Upload from local directory
    Import from GitHub repository
    Skip (use existing source)

? Directory to upload: ./dist
  ✓ Found package.json
  ✓ 47 files, ~2.3 MB

  Uploading source...
  ████████████████████████████████ 100%

  ⠸ Building... (job: job-789)
  ⠼ Installing dependencies...
  ⠴ Running build script...
  ✓ Build complete (38s)

? What next?
  ❯ Publish to production
    Keep as preview only
    View build logs first

  Publishing deployment...
  ✓ Deployed to production

  ┌──────────────────────────────────────┐
  │ Deployment Summary                   │
  ├──────────────────────────────────────┤
  │ App:       my-node-app               │
  │ Status:    live                      │
  │ Source:    local upload (47 files)    │
  │ Build:     38s                       │
  │ URL:       my-node-app.godaddy.app   │
  └──────────────────────────────────────┘

  Next steps:
  → gddy hosting nodejs status --app-id abc-123
  → gddy hosting nodejs logs --app-id abc-123 \
      --target publish --source stdout --since now
```

**Reuse:** Calls the same `HostingClient` methods as existing commands. Job polling logic mirrors `is_source_job_terminal` / `is_app_creation_job_terminal` helpers already in `hosting/nodejs/mod.rs`.

---

### 5. Email Mailbox Provisioning — `gddy email setup` (MEDIUM priority)

**Current pain points:**
- `check-eligibility` returns account IDs that must be manually copied to `create`
- No guidance on which account to choose when multiple are eligible
- Consent requirements are checked but not actionable in the flow
- Users must read the guide to understand what an "account ID" means

**Interactive flow (4 steps):**

| Step | Prompt Type | Description |
|------|-------------|-------------|
| Email Address | Validated Text | Enter desired email address; validate format; extract domain |
| Eligibility Check | Auto + Display | Run check-eligibility; show eligible accounts with plan details |
| Account Selection | Select | Pick account if multiple eligible; show name, plan, remaining slots |
| Consent & Create | Confirm | Show consent requirements; confirm; display result with login info |

**Terminal mockup:**

```
$ gddy email setup

  Email Mailbox Setup
  ───────────────────

? Email address: hello@example.com

  Checking eligibility for example.com...
  ✓ 1 eligible account found

  ┌────────────────────────────────────────┐
  │ Account: Business Email Professional   │
  │ ID:      acct-xyz-789                  │
  │ Slots:   3 of 5 used                   │
  │ Domain:  example.com                   │
  └────────────────────────────────────────┘

? Create mailbox hello@example.com? (Y/n) Yes

  ✓ Mailbox created

  ┌────────────────────────────────────────┐
  │ hello@example.com                      │
  │ Status: active                         │
  │ Webmail: https://email.godaddy.com     │
  └────────────────────────────────────────┘

  Next steps:
  → gddy email get <mailbox-id>
  → gddy email list
```

---

### 6. Interactive API Call Builder — `gddy api explore` (MEDIUM priority)

**Current pain points:**
- `api call` requires knowing the exact endpoint path, method, and body format
- Discovery chain is 4 commands: `domain list` → `operation list` → `operation get` → `call`
- Parameter details must be read from `operation get` output and manually composed
- GraphQL calls need compound operation ID format (`domain::kind::name`)
- `--body` flag requires valid JSON strings that are hard to compose on the command line

**Interactive flow (6 steps):**

| Step | Prompt Type | Description |
|------|-------------|-------------|
| API Domain | Searchable Select | List all API domains from catalog with descriptions |
| Operation | Filtered Select | List operations; filter by method or search by keyword |
| Parameters | Dynamic Form | Type-appropriate input per required param; optional params as loop |
| Request Body | Guided JSON | POST/PUT: prompt field-by-field from schema; fallback to raw JSON |
| Dry Run Preview | Display + Confirm | Full request preview (method, URL, headers, body); edit or confirm |
| Execute & Display | Spinner + Result | Send request; show response; offer save or run-another |

**Terminal mockup:**

```
$ gddy api explore

  API Explorer
  ────────────

? Select an API domain:
  ❯ domains         Manage domains and DNS
    catalog-products Product catalog API
    orders           Order management
    taxes            Tax calculation
    hosting          Hosting management
    (search: _)

  Selected: domains
? Select an operation:
    GET  /v3/domains                    List domains
  ❯ GET  /v3/domains/{domain}           Get domain details
    POST /v3/domains/available          Check availability
    POST /v3/domains/suggest            Suggest domains
    GET  /v3/domains/{domain}/records   List DNS records
    (filter: _)

  Selected: GET /v3/domains/{domain}

  Parameters:
? domain (required, path): example.com

  Optional parameters:
? Add optional parameters? (y/N) Yes
? includes (query, multi-value):
  ◉ contacts
  ◯ nameServers
  ◯ dnsSec

  Request Preview:
  ┌──────────────────────────────────────────┐
  │ GET /v3/domains/example.com?includes=... │
  │ Authorization: Bearer ****               │
  │ Accept: application/json                 │
  └──────────────────────────────────────────┘

? Send request? (Y/n) Yes

  ✓ 200 OK (143ms)

  {
    "domain": "example.com",
    "status": "ACTIVE",
    "expires": "2027-03-15T00:00:00Z",
    ...
  }

? What next?
  ❯ Run another request
    Save response to file
    Done
```

**Reuse:** Catalog browsing uses the existing `ApiCatalog` from `api_explorer/catalog.rs`. Request execution reuses the `api call` handler's HTTP client and auth injection. `--dry-run` preview uses the same dry-run infrastructure.

---

## Architecture

### Wizard Framework Pattern

All wizards follow the established `domain register` pattern:

```
command entry point
    │
    ├── interactive? ──► WizardState::new() + pre-fill from flags
    │                         │
    └── non-interactive ──────┤
                              ▼
                      run_wizard(state, ctx, start_at)
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
          step_0(state)   step_1(state)   step_N(state)
              │               │               │
              └───────────────┼───────────────┘
                              ▼
                     StepResult dispatch:
                     Continue → next step
                     Back → previous step
                     Cancel → abort
```

### Prompt Primitives

**Existing in cli-engine (`prompt.rs`):**

| Function | Type | Usage |
|----------|------|-------|
| `prompt_text` | Free text | Domain names, URLs, descriptions |
| `prompt_text_with_validation` | Validated text | Email, IP, app names |
| `prompt_select` | Single choice | Record types, datacenters, actions |
| `prompt_confirm` | Yes/no | Confirmations, toggles |
| `prompt_multi_select` | Multiple choice | Scopes, events, parameters |
| `try_recover_missing_args` | Auto-recovery | Missing required flags |

**New primitives needed:**

1. **Searchable Select** — `inquire::Select` with filter enabled. Required for API domains (~40 items), webhook events (~20), and OAuth scopes (~30).

2. **Spinner / Progress** — Standardized wrapper around `indicatif::ProgressBar`. The domain wizard uses `indicatif` directly; this should be a shared cli-engine helper for consistency across hosting job polling, domain registration, and API calls.

3. **Inline Table Display** — Render a formatted table to stderr mid-wizard (using `comfy-table` or similar) without ending the command. Used for showing DNS records, app lists, and deployment history before prompting.

### Entry Point Design

Each wizard is a **new command** (e.g., `dns manage`, `platform app bootstrap`) rather than an `--interactive` flag on existing commands. This keeps atomic commands stable for scripts and agents.

**Bridge pattern**: Related commands offer to enter the wizard when running interactively. For example, `dns list` in interactive mode can offer "Edit these records?" which bridges into `dns manage` at the appropriate step.

### Non-Interactive Fallback

Every wizard must work with `--non-interactive` by accepting all parameters as flags. CI pipelines and AI agents use the flag path; humans get prompts. The `try_recover_missing_args` fallback in cli-engine handles the middle ground: missing flags trigger prompts when running interactively.

### Agent Compatibility

Wizards detect agent mode via `ctx.is_interactive()` and skip all prompts. `next_actions` in command output already guide agents through the atomic command chain. Wizards are a human-only overlay on the same underlying API calls.

### State Persistence

- **In-memory `WizardState`**: Short wizards (DNS, email, API explorer) that complete in a single session.
- **File-backed cache**: Long wizards with payment gates or multi-session flows (platform bootstrap, hosting deploy) — following the `quote_cache` pattern from `domain register`.

---

## Priority Matrix

| Wizard | Priority | Steps | Key Driver |
|--------|----------|-------|------------|
| DNS Record Manager | HIGH | 6 | No preview before destructive operations |
| Platform App Bootstrap | HIGH | 7 | 6+ chained commands with ID copy-paste |
| Hosting Deploy Wizard | HIGH | 6 | Job ID polling across 3+ commands |
| DNS Batch Editor | MEDIUM | 3 | No batch staging for multiple record changes |
| Email Mailbox Setup | MEDIUM | 4 | Account ID copy-paste from eligibility check |
| API Call Builder | MEDIUM | 6 | 4-command discovery chain; hard JSON composition |

### Commands Excluded from Wizards

| Group | Reason |
|-------|--------|
| `env` | Simple get/set; only 3 leaf commands |
| `pat` | add/list/remove are each one-step operations |
| `update` | check + apply is already two simple steps |
| `platform actions` | Read-only catalog browse |
| `platform webhook` | Single read-only command |
| `auth` | Login already opens browser (PKCE); status/logout are one-shot |

---

## Implementation Roadmap

### Phase 1: Foundation (1-2 weeks)
- Enable `auto_interactive` in `main.rs` (currently commented out)
- Standardize on cli-engine prompts instead of raw `dialoguer`
- Add searchable select variant to cli-engine prompt module
- Add spinner/progress bar helpers to cli-engine

### Phase 2: DNS Interactive (2-3 weeks)
- `gddy dns manage` — unified interactive DNS manager
- `gddy dns edit <zone>` — batch zone editor with diff preview
- Leverage existing `--dry-run` on `set`/`delete` for preview step

### Phase 3: Platform Bootstrap (3-4 weeks)
- `gddy platform app bootstrap` — full app creation wizard
- Integrate webhook events fetch into subscription sub-wizard
- Chain init → add components → validate → release

### Phase 4: Hosting & API (2-3 weeks)
- `gddy hosting nodejs deploy-wizard` — create/upload/poll/publish
- `gddy api explore` — interactive API call builder
- Auto-polling for async jobs (app creation, source upload)

### Phase 5: Email & Polish (1-2 weeks)
- `gddy email setup` — eligibility → create wizard
- Global wizard back/cancel/resume behavior
- Non-interactive flag bypass for all wizards (CI/agent compatibility)

**Total estimated effort: 9-14 weeks**
