# GoDaddy Apps — CLI (PROD) vs Developer Docs Discrepancies

**Scope:** CLI source on `main` branch (PROD release) compared against developer docs at `developer.test-godaddy.com/en/docs/api-users/commerce/apps/guides/applications`  
**CLI Source:** `https://github.com/godaddy/cli/tree/main/rust`  
**Date:** 2026-09-09  
**Reviewer:** Claude (prompted by Rajkumar TS)

---

## Table of Contents

1. [Command Name & Syntax Discrepancies](#1-command-name--syntax-discrepancies)
2. [Missing / Undocumented CLI Commands](#2-missing--undocumented-cli-commands)
3. [Flag & Argument Discrepancies](#3-flag--argument-discrepancies)
4. [Config Schema Discrepancies](#4-config-schema-discrepancies)
5. [Settings System — Entirely Undocumented](#5-settings-system--entirely-undocumented)
6. [Lifecycle & Status Discrepancies](#6-lifecycle--status-discrepancies)
7. [Validation Behavior Discrepancies](#7-validation-behavior-discrepancies)
8. [URL Validation Discrepancies](#8-url-validation-discrepancies)
9. [Credential & Env File Discrepancies](#9-credential--env-file-discrepancies)
10. [Webhook Command Discrepancies](#10-webhook-command-discrepancies)
11. [Workflow / Sequencing Discrepancies](#11-workflow--sequencing-discrepancies)
12. [Undocumented CLI Features](#12-undocumented-cli-features)
13. [Summary Table](#13-summary-table)

---

## 1. Command Name & Syntax Discrepancies

### DISC-01: `gddy platform app init` Flags Don't Match Docs

**Docs say:**
```bash
gddy platform app init \
  --name my-app \
  --description "My application" \
  --url https://example.com \
  --proxy-url https://example.com \
  --scopes commerce.order:read
```

**CLI actually accepts (PROD `main:rust/src/application/commands/init.rs`):**

| Flag | Docs | CLI (PROD) | Match? |
|------|------|------------|--------|
| `--name` / `-n` | `--name` | `--name` / `-n` | Docs omit short `-n` |
| `--description` | `--description` | `--description` | OK |
| `--url` | `--url` | `--url` | OK |
| `--proxy-url` | `--proxy-url` | `--proxy-url` | OK |
| `--scopes` | `--scopes` | `--scopes` | OK |
| `--label` | Not documented | `--label` | **Missing from docs** |
| `-c` / `--config` | Not documented | `-c` / `--config` | **Missing from docs** |
| `--accept-agreements` | `--accept-agreements` | `--accept_agreements` | OK (clap normalizes) |
| `--human` | Documented in docs | Not a flag — cli-engine `--output human` | **Wrong in docs** |

**Impact:** The `--label` flag lets developers set a merchant-facing display name different from the app name. The `--config` flag lets you read defaults from an existing TOML. Both are useful features invisible to anyone reading the docs.

### DISC-02: `gddy platform app enable` / `disable` Syntax Mismatch

**Docs say:**
```bash
gddy platform app enable my-app --store-id test-store-123
gddy platform app disable my-app --store-id test-store-123
```

This matches the CLI (`lifecycle.rs:12-19`): positional `NAME` + `--store-id STORE_ID`. **This is consistent.**

However, the "About Apps" overview page describes testing as:
```bash
gddy platform app enable [name]
gddy platform app disable [name]
```
without the required `--store-id` flag. The `--store-id` is **required** per `lifecycle.rs:19` (`#[arg(long = "store-id")]`).

**Impact:** A developer following the overview page's examples will get a missing-argument error.

### DISC-03: `gddy platform app deploy` Flag Mismatch

**Docs say:**
```bash
gddy platform app deploy --name my-app
```

**CLI (PROD `main:rust/src/application/commands/deploy/mod.rs`):**
```rust
#[arg(long, short = 'n', value_name = "NAME")]
name: String,
```

The CLI also accepts `-n` (short form). Docs don't mention it. Minor, but the short form is convenient.

### DISC-04: `gddy platform app release` Flag vs Docs

**Docs say:**
```bash
gddy platform app release --application-id <APPLICATION_ID> --version <SEMANTIC_VERSION>
```

**CLI (PROD `main:rust/src/application/commands/release.rs:59-72`):**
```rust
#[arg(long = "application-id", value_name = "ID")]
application_id: String,

#[arg(long, value_name = "VERSION")]
version: String,

#[arg(long, value_name = "TEXT")]
description: Option<String>,
```

**Discrepancy:** The `--description` flag exists in PROD but is not documented.

---

## 2. Missing / Undocumented CLI Commands

### DISC-05: `gddy platform app archive` — Not Documented

The PROD CLI has an `archive` command (`lifecycle.rs:103-146`) that permanently archives an application. The docs mention only INACTIVE and ACTIVE states. The archive command is a **destructive, irreversible** operation (`Tier::Destructive`) that the docs don't even hint at.

**Impact:** Developers don't know they can archive apps, and can't find documentation on the consequences (e.g., does it break existing installations?).

### DISC-06: `gddy platform app update` — Not Documented

The PROD CLI has an `update` command (`update.rs`) that changes an app's label and description by application ID.

**PROD CLI accepts:**
```bash
gddy platform app update --id <ID> --label <LABEL> --description <TEXT>
```

**IMPORTANT:** PROD explicitly does **NOT** accept `--status`. There is a test (`status_is_an_unsupported_argument`) that verifies `--status` is rejected. Status changes are handled via `enable`/`disable` (for ACTIVE/INACTIVE) and `archive` (for ARCHIVED).

**Impact:** Developers who want to change app metadata think they need the dashboard. They can do it from the CLI (but only for label and description — not status).

### DISC-07: `gddy platform app list` — Not Documented in App Guides

The CLI has a `list` command that shows all applications across statuses. The docs reference it indirectly in error messages ("Use `gddy platform app list`") but never document the command itself.

### DISC-08: `gddy platform app add action` — Not Documented

The PROD CLI has `platform app add action --name <name> --url <url>` to append action entries to `godaddy.toml`. The docs don't mention actions at all — only webhook subscriptions and OAuth scopes.

### DISC-09: `gddy platform app add extension` — Not Documented

The PROD CLI has a full `add extension` subgroup with `embed`, `checkout`, and `blocks` subcommands. The docs don't mention UI extensions anywhere in the Apps guides. The deploy command bundles and uploads extensions, but the docs describe deploy only in terms of releasing.

### DISC-10: `gddy platform app add subscription` — Partially Documented

The webhooks guide mentions using `gddy platform webhook events` to list event types and configuring subscriptions in `godaddy.toml`, but doesn't document the `gddy platform app add subscription` command that automates this:

```bash
gddy platform app add subscription --name <name> --url <url> --events <event1> <event2>
```

### DISC-11: `gddy platform app add settings` — Not Documented

**This is a significant feature present in PROD but completely absent from the developer docs.**

The PROD CLI has `platform app add settings` (`main:rust/src/application/commands/add.rs:171-237`) which registers a settings placement entry in `godaddy.toml`:

```bash
gddy platform app add settings \
  --group <GROUP_SLUG> \
  --slug <SETTING_SLUG> \
  --title <TITLE> \
  --description <DESCRIPTION> \
  --entry-path <PATH> \
  --order <NUMBER> \
  --capability read --capability write \
  --icon-name <NAME> \
  --icon-library <LIBRARY> \
  --presentation-file <JSON_PATH>
```

**Flags:**

| Flag | Required | Description |
|------|----------|-------------|
| `--group` | Yes | Commerce-owned settings group slug |
| `--slug` | Yes | Unique setting slug |
| `--title` | No | Display title |
| `--description` | No | Display description |
| `--entry-path` | Yes | GPA settings namespace path |
| `--order` | No | Sort order within the group |
| `--capability` | No | Multiple allowed: `read`, `write`, `validate`, `test`, `delete` |
| `--icon-name` | No | Icon name |
| `--icon-library` | No | Icon library: `ux`, `lucide`, `commerce` |
| `--presentation-file` | No | Path to JSON file with `settings-form-v1` presentation |

**Impact:** Developers cannot discover the settings system from the docs at all.

### DISC-12: `gddy platform app config validate` — Not Documented

**The PROD CLI has a `config validate` command (`main:rust/src/application/commands/config.rs`) that is entirely absent from the developer docs.**

```bash
gddy platform app config validate
```

This command:
- Validates the local `godaddy.toml` (or `godaddy.<env>.toml`) against the schema
- Requires **no authentication** (`no_auth(true)`) — works fully offline
- Returns structured output: `{valid: bool, errors: [], warnings: []}`
- Catches all config validation errors (name pattern, UUID format, semver, URL format, settings validation, etc.)

The docs mention "Local TOML fields against schema" validation, but attribute it to the `validate` command (which only checks remote state). The `config validate` command is a separate, dedicated command for local validation.

**Impact:** Developers don't know they can validate their manifest offline before any network calls.

---

## 3. Flag & Argument Discrepancies

### DISC-13: `--human` Flag Doesn't Exist

**Docs say:**
```bash
gddy platform app info --name my-app --human
gddy platform webhook events --human
```

**CLI reality:** There is no `--human` flag anywhere in the source. The cli-engine provides `--output human` (or `--output json`). The docs reference `--human` in the init and webhook events examples, but this flag is handled by cli-engine's output format system, not by individual commands.

If `--human` is a cli-engine global alias for `--output human`, the docs should say so explicitly. If it's not, the examples are broken.

### DISC-14: `gddy platform app validate` Takes Positional Name, Not `--name`

**Docs say:**
```bash
gddy platform app validate <APPLICATION_NAME>
```

**CLI (`validate.rs:30-33`):**
```rust
#[arg(value_name = "NAME")]
pub(super) name: String,
```

This is a positional argument, not `--name`. The docs are correct here, but inconsistent with `info` which uses `--name` (a named flag). This inconsistency is between CLI commands, not between docs and CLI — but the docs don't call it out.

### DISC-15: `gddy platform app info` Uses `--name`, Not Positional

`info` uses `--name` (`#[arg(long, short = 'n')]`), while `validate`, `enable`, and `disable` use positional arguments. The docs show `info --name <name>` correctly, but don't explain why some commands use positional and others use flags.

---

## 4. Config Schema Discrepancies

### DISC-16: `godaddy.toml` Has Fields Not in Docs

**CLI Config struct (PROD `main:rust/src/config/mod.rs`):**

| Field | In Docs | In PROD CLI |
|-------|---------|-------------|
| `name` | Yes | Yes |
| `client_id` | Yes | Yes |
| `description` | Yes | Yes |
| `version` | Yes | Yes |
| `url` | Yes | Yes |
| `proxy_url` | Yes | Yes |
| `authorization_scopes` | Yes | Yes |
| `actions` | **No** | Yes (`Vec<ActionConfig>`) |
| `subscriptions.webhook` | Yes | Yes |
| `dependencies` | **No** | Yes (`Vec<DependenciesConfig>`) |
| `extensions` | **No** | Yes (`Option<ExtensionsConfig>`) |
| `settings` | **No** | Yes (`Vec<SettingConfig>`) |

**Missing from docs:**
- `actions` — HTTP endpoints the platform invokes (name + URL pairs)
- `dependencies` — app and feature dependencies with optional version constraints
- `extensions` — embed, checkout, and blocks UI extensions with handle, source, and target
- `settings` — settings placement metadata with group, slug, entry_path, capabilities, icon, and presentation

**Impact:** Four entire feature categories (`actions`, `dependencies`, `extensions`, `settings`) are invisible to developers reading the docs. The `settings` system is particularly significant — it has its own CLI commands, presentation format, validation logic, and release-time processing.

### DISC-17: Subscription URL Can Be Relative

**Docs show** absolute URLs in webhook subscription examples:
```toml
url = "https://yourapp.com/webhooks/orders"
```

**CLI (`config/mod.rs`)** validates subscription URLs as "endpoint URLs" resolved against `proxy_url`:
```rust
fn is_endpoint_url(endpoint: &str, proxy_url: &str) -> bool {
    let Ok(base) = url::Url::parse(proxy_url) else { return false; };
    url::Url::options().base_url(Some(&base)).parse(endpoint)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
}
```

So `/webhooks/orders` (relative path) is valid when `proxy_url` is set. The setup page shows this correctly in one example but the webhooks guide only shows absolute URLs.

### DISC-18: `description` Is Optional in CLI, Sometimes Required in Docs

**CLI (`config/mod.rs`):** `description: Option<String>` (optional, defaults to `None`)

**CLI init (`init.rs`):** Description is required during init (returns error if empty).

**Docs setup page:** Lists description as required during init but optional in the config reference.

This is internally consistent between init and config, but the config reference should clarify that `description` is optional in the TOML but required during `init`.

---

## 5. Settings System — Entirely Undocumented

**This is the single largest gap between the PROD CLI and the developer docs. The entire settings system exists in production but has zero developer-facing documentation.**

### DISC-19: Settings Config Schema

The PROD `godaddy.toml` supports a `[[settings]]` array with the following structure (`main:rust/src/config/settings.rs`):

```toml
[[settings]]
group = "godaddy-tax"
slug = "tax-configuration"
title = "Tax Configuration"
description = "Configure tax settings"
entry_path = "/settings/godaddy-tax"
order = 1
capabilities = ["read", "write", "validate"]

[settings.icon]
name = "settings"
library = "ux"

# Option A: inline presentation
[settings.presentation]
# ... settings-form-v1 structure ...

# Option B: external file reference
# presentation_file = "settings-presentation.json"
```

**Validation rules (PROD):**
- `group`: must match slug pattern `^[a-z0-9]+(-[a-z0-9]+)*$`
- `slug`: must match slug pattern `^[a-z0-9]+(-[a-z0-9]+)*$`
- `entry_path`: must start with `/`, no scheme/query/fragment/`..` segments, route-safe characters
- `capabilities`: must be from allowed set: `read`, `write`, `validate`, `test`, `delete`
- `icon.library`: must be from allowed set: `ux`, `lucide`, `commerce`
- Entry path overlap detection: no two settings can have overlapping entry paths

None of this is documented.

### DISC-20: `settings-form-v1` Presentation System

The PROD CLI has a complete `settings-form-v1` presentation system (`main:rust/src/config/settings_form.rs`, 479 lines) that defines how settings forms are rendered:

**Field types:**

| Type | Properties |
|------|-----------|
| `text` | key, label, description, required, placeholder, minLength, maxLength, defaultValue |
| `textarea` | key, label, description, required, placeholder, minLength, maxLength, defaultValue |
| `number` | key, label, description, required, min, max, defaultValue |
| `boolean` | key, label, description, defaultValue |
| `select` | key, label, description, required, options, defaultValue |
| `multi-select` | key, label, description, required, options, defaultValue |
| `list-group` | key, label, description, required, minItems, maxItems, fields (nested) |

**Structure:**
```json
{
  "sections": [
    {
      "key": "section-1",
      "label": "General Settings",
      "description": "Optional section description",
      "visibleWhen": { "field": "some-field", "equals": "some-value" },
      "fields": [
        { "type": "text", "key": "field-1", "label": "Field Label", "required": true }
      ]
    }
  ]
}
```

**Features:**
- Conditional visibility (`visibleWhen`) on sections
- Nested fields in `list-group` type (one level deep)
- `presentationFile` — external JSON file reference, resolved at release time
- Validation: unique keys across all sections/fields, section must have at least one field
- `schemaVersion: "settings-form-v1"` injected automatically at release time

None of this is documented in the developer docs.

### DISC-21: Release Command Handles Settings Resolution

The PROD `release` command (640 lines, `main:rust/src/application/commands/release.rs`) includes significant settings processing:

1. **`resolve_presentation()`** — resolves a setting's presentation from inline `presentation` or external `presentationFile`
2. **`setting_entry()`** — builds each setting's release entry, validates the presentation, and injects `schemaVersion`
3. **Mutual exclusivity** — a setting cannot have both `presentation` and `presentationFile`
4. **Hard rejection** — a setting with neither `presentation` nor `presentationFile` is rejected at release time (not at config validation time, so `add settings` can write a placement-only entry)

The docs describe `release` as simply creating a versioned snapshot. In reality, it performs complex settings resolution and validation.

### DISC-22: Platform Guides Document Settings (But Are Undocumented Themselves)

The PROD platform module embeds two guides (`main:rust/src/platform/mod.rs`):
- `platform-overview.md` — complete lifecycle guide: init → add components → release → deploy → enable/disable → archive
- `platform-settings.md` — detailed settings-form-v1 guide with workflow, presentation shape, field types, gotchas

These guides are available via `gddy platform --guides` (a cli-engine feature) but:
1. The developer docs don't mention that CLI guides exist
2. The developer docs don't reproduce or link to the information in these guides
3. The `platform-settings.md` guide contains critical information (no release inheritance, existing stores don't auto-upgrade) that exists nowhere else

---

## 6. Lifecycle & Status Discrepancies

### DISC-23: Five App Statuses vs Three Documented

**CLI (`client.rs`):**
```rust
pub const APPLICATION_STATUSES: &[&str] =
    &["ACTIVE", "ARCHIVED", "BLOCKED", "INACTIVE", "VERIFYING"];
```

**Docs mention:**
- **INACTIVE** (new release)
- **ACTIVE** (deployed)
- **Suspended** (compliance failure — mentioned only on the dashboard page)

**Missing from docs:**
- **ARCHIVED** — reachable via `gddy platform app archive` (undocumented command)
- **BLOCKED** — presumably a platform-enforced state, no documentation
- **VERIFYING** — presumably a transitional compliance state, no documentation

**Impact:** Developers encountering BLOCKED or VERIFYING status in API responses or `app list` output have no documentation to explain what happened or how to resolve it.

### DISC-24: `update` Does NOT Accept `--status`

**PROD CLI (`main:rust/src/application/commands/update.rs`):**

The update command accepts ONLY:
- `--id <APPLICATION_ID>` (required)
- `--label <LABEL>` (optional)
- `--description <TEXT>` (optional)

There is an explicit test (`status_is_an_unsupported_argument`) that verifies `--status` is rejected:
```rust
fn status_is_an_unsupported_argument() {
    // ...try_get_matches_from(["update", "--id", "app-1", "--status", "ACTIVE"])
    //   .expect_err("--status must not be accepted by app update");
}
```

Status transitions are handled by separate commands:
- `enable` / `disable` → ACTIVE / INACTIVE
- `archive` → ARCHIVED

The docs don't document `update` at all, so this isn't a docs-vs-CLI discrepancy per se, but any future documentation must correctly reflect that `update` is metadata-only.

---

## 7. Validation Behavior Discrepancies

### DISC-25: Docs Conflate `validate` and `config validate`

**Docs say:**
> "Validation checks: Remote application state (URL configured, app active), Local TOML fields against schema, Automatic TOML parsing on every file-reading CLI command"

**PROD CLI reality — two separate commands:**

1. **`gddy platform app validate <NAME>`** (`validate.rs`) — checks **remote** state only:
   - URL is not empty (error)
   - Proxy URL is not empty (warning)
   - Status is not INACTIVE (warning)

2. **`gddy platform app config validate`** (`config.rs`) — checks **local** TOML only:
   - All config validation rules (name pattern, UUID, semver, URL format, settings validation)
   - No auth required — works fully offline
   - Returns `{valid, errors, warnings}`

The docs attribute both behaviors to a single `validate` command. In reality, remote validation and local validation are separate commands. Furthermore, the `config validate` command is not documented at all.

### DISC-26: Config Validation Rules Not Documented

The CLI validates (`config/mod.rs` + `config/settings.rs`):
- `name`: must match `/^[a-z0-9-]{3,255}$/`
- `client_id`: must be UUID v4
- `version`: must be semver
- `url`: must be absolute HTTP(S) URL
- `proxy_url`: must be absolute HTTP(S) URL
- `authorization_scopes`: must not be empty
- `actions[].name`: min 3 characters
- `actions[].url`: valid endpoint URL relative to proxy_url
- `subscriptions.webhook[].name`: min 3 characters
- `subscriptions.webhook[].events`: must not be empty
- `subscriptions.webhook[].url`: valid endpoint URL relative to proxy_url
- `dependencies[].name`: min 3 characters
- `dependencies[].version`: must be semver if present
- `extensions.embed[]/checkout[].name`: min 3 characters
- `extensions.embed[]/checkout[].handle`: min 3 characters
- `extensions.embed[]/checkout[].source`: must be non-empty
- `extensions.embed[]/checkout[].targets`: must have at least one target
- `extensions.blocks.source`: must be non-empty
- `settings[].group`: must match slug pattern
- `settings[].slug`: must match slug pattern
- `settings[].entry_path`: must be valid route path starting with `/`
- `settings[].capabilities`: must be from `[read, write, validate, test, delete]`
- `settings[].icon.library`: must be from `[ux, lucide, commerce]`
- Settings entry-path overlap detection

The docs mention the name pattern and that `client_id` is UUID v4, but don't document most other validation rules — particularly the entire settings validation section.

---

## 8. URL Validation Discrepancies

### DISC-27: CLI Rejects HTTP URLs at Init, Docs Config Reference Allows HTTP

**CLI init (`init.rs`):**
```rust
if !crate::application::public_url::is_public_routable_url(u) {
    return Err(cli_engine::CliCoreError::message(format!(
        "Invalid application configuration: {field} must be a publicly-resolvable \
         http(s) URL (localhost, loopback, and private IPs are not allowed)"
    )));
}
```

**CLI `public_url.rs`:** Accepts both `http` and `https` schemes but rejects localhost, loopback, private IPs, CGNAT, and link-local addresses.

**Docs config reference:** `url` requires "Publicly routable HTTP or HTTPS URL"
**Docs setup page:** `url` requires "Publicly routable HTTPS URLs"

**Actual behavior:** The CLI accepts HTTP but the security best practices say HTTPS-only. The CLI and the config reference agree on allowing HTTP; the setup page disagrees.

### DISC-28: CLI Config Validation Accepts HTTP Too

**CLI (`config/mod.rs`):**
```rust
fn is_absolute_http_url(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
}
```

The config validator accepts both HTTP and HTTPS. It does NOT check for public routability (unlike `init`). So you can write `http://10.0.0.1` into `godaddy.toml` and `config validate` will accept it, but `init` would reject it.

**Impact:** Validation inconsistency — `init` enforces public-routability, but editing the TOML directly bypasses it.

---

## 9. Credential & Env File Discrepancies

### DISC-29: `.env` Contains Four Keys, Docs Show Three

**CLI (`config/mod.rs`):**
```rust
let owned = [
    ("GODADDY_WEBHOOK_SECRET", secret),
    ("GODADDY_PUBLIC_KEY", public_key),
    ("GODADDY_CLIENT_ID", client_id),
    ("GODADDY_CLIENT_SECRET", client_secret),
];
```

**Docs show three env vars:**
```bash
GODADDY_CLIENT_ID=<CLIENT_ID>
GODADDY_CLIENT_SECRET=<CLIENT_SECRET>
GODADDY_WEBHOOK_SECRET=<WEBHOOK_SECRET>
```

**Missing from docs:** `GODADDY_PUBLIC_KEY` — written by `init` but never referenced in the docs. Developers won't know what it's for or whether they need it.

### DISC-30: Init Returns `publicKey` — Docs Don't Mention It

**CLI (`init.rs`):**
```rust
let public_key = app["publicKey"].as_str().unwrap_or("").to_owned();
```

The GraphQL mutation `createApplication` returns `publicKey` and it's saved to `.env`, but the docs only mention Client ID, Client Secret, and Webhook Secret as credentials.

The dashboard page mentions "Public Key: Non-confidential identifier shown at creation" but the guides never explain its purpose or when to use it.

### DISC-31: Env Values Are JSON-Encoded

**CLI (`config/mod.rs`):**
```rust
fn format_env_value(value: &str) -> String {
    serde_json::to_string(&value.replace('\0', "")).unwrap_or_default()
}
```

Values in the `.env` file are written as JSON strings (quoted, with escaping). The docs show bare values:
```bash
GODADDY_CLIENT_ID=<CLIENT_ID>
```

But the CLI actually writes:
```bash
GODADDY_CLIENT_ID="<CLIENT_ID>"
```

Most `.env` parsers handle both, but this is a subtle difference that could bite someone hand-parsing the file.

---

## 10. Webhook Command Discrepancies

### DISC-32: `gddy platform webhook events` — No `--human` Flag

**Docs say:**
```bash
gddy platform webhook events --human
```

**CLI (PROD `main:rust/src/webhook/mod.rs`):** The command accepts no custom flags at all. It's a simple `CommandSpec::new("events", ...)` with no args struct. The `--human` is presumably cli-engine's `--output human`, not a command-specific flag. The docs should use `--output human` or explain the alias.

### DISC-33: Webhook Events Are Truncated at 50 — Not Documented

**CLI (`webhook/mod.rs:35`):**
```rust
const MAX_LIST_ITEMS: usize = 50;
```

If more than 50 event types exist, the output is truncated and a `full_output` field points to a temp file with the complete list. This behavior is in the command's `--long` help text but not in the developer docs.

---

## 11. Workflow / Sequencing Discrepancies

### DISC-34: Release + Deploy Are Separate Commands — Docs Conflate Them

**Docs say:**
```bash
gddy platform app release --application-id <APPLICATION_ID> --version <SEMANTIC_VERSION>
gddy platform app deploy --name my-app
```

The docs show these as a two-step process (correct), but the overview page describes them as one step: "Releases are versioned snapshots created in INACTIVE state. Deployment activates them."

**CLI reality:**
1. `release` creates a versioned snapshot (INACTIVE) — takes `--application-id` + `--version`. **In PROD, this also captures actions, webhook subscriptions, UI extensions, AND settings from `godaddy.toml` into the release.**
2. `deploy` reads `godaddy.toml`, bundles extensions, uploads artifacts, activates the release, then promotes the app to ACTIVE — takes `--name`

The deploy command does much more than "activate" — it bundles JavaScript extensions, runs a security scanner (SEC101-SEC115), uploads artifacts to S3, then activates. None of this is documented.

**Additionally:** The PROD release command (640 lines) does significant processing that the docs don't describe:
- Reads actions, subscriptions, and UI extensions from TOML and includes them in the release
- Resolves settings presentations (inline or from external files)
- Validates settings presentations
- Injects `schemaVersion: "settings-form-v1"` into each settings presentation
- Rejects settings with no presentation (neither inline nor file)
- Enforces one-target-per-extension limit

### DISC-35: Deploy Requires a Release — Error Message Not in Docs

**CLI (`deploy/mod.rs`):**
```rust
"application '{name}' has no releases — create one first with: \
 gddy platform app release --application-id {application_id} --version 0.0.1"
```

If you try to deploy without creating a release first, you get this error. The docs don't explain this ordering requirement clearly — they mention it in passing but don't show the error or recovery.

### DISC-36: Settings Have No Release Inheritance

**PROD platform guide (`platform-settings.md`):**
> Settings have no inheritance across releases. Each release captures its own settings snapshot.

This means if you release v1.0.0 with settings, then release v1.1.0 without re-specifying settings, v1.1.0 has NO settings — not the ones from v1.0.0. This gotcha is documented in the embedded CLI guide but not in the developer docs.

### DISC-37: Existing Stores Don't Auto-Upgrade

**PROD platform guide (`platform-settings.md`):**
> Stores already running a release don't automatically upgrade to a new one.

This deployment behavior gotcha is documented in the embedded CLI guide but not in the developer docs. Developers might assume that deploying a new release automatically rolls it out to all stores.

---

## 12. Undocumented CLI Features

### DISC-38: GraphQL API — Not Documented

The CLI communicates with the App Registry via a GraphQL API at `/v1/apps/app-registry-subgraph` (`client.rs:5`). The docs never mention GraphQL — they refer to "the Applications API" generically. Developers building their own tooling or debugging would benefit from knowing the API is GraphQL-based.

### DISC-39: Security Scanner in Deploy — Not Documented

The `deploy` command runs a security scanner (`SEC101-SEC115`) on bundled extensions (`deploy/mod.rs`). This can block deployment if blocking rules are triggered. Developers have no documentation on:
- What rules exist
- Which rules block vs warn
- How to fix violations
- How to test locally before deploying

### DISC-40: Extension Bundling with esbuild — Not Documented

Deploy bundles extensions with esbuild, which must be available via `node_modules/.bin/esbuild` or PATH. This runtime dependency is mentioned in `AGENTS.md` but not in the developer-facing docs.

### DISC-41: `next_actions` System — Not Documented

Every CLI command returns `next_actions` — suggested follow-up commands. This is a powerful UX feature that the docs don't mention. Understanding the suggested workflow could help developers learn the platform faster.

### DISC-42: `--output` Format Flag — Not Documented in App Guides

The CLI supports `--output json`, `--output human`, and presumably other formats via cli-engine. The docs don't explain output formatting options. The `--human` references in the docs appear to be an incorrect shorthand for `--output human`.

### DISC-43: Embedded Platform Guides — Not Documented

The PROD CLI embeds two Markdown guides accessible via `gddy platform --guides` (a cli-engine feature):
- `platform-overview.md` — complete lifecycle walkthrough
- `platform-settings.md` — settings-form-v1 authoring guide

These contain information not available anywhere in the developer docs, including:
- The `config validate` command
- The `add settings` command
- Settings presentation shape and field types
- Release behavior (no settings inheritance)
- Deploy behavior (existing stores don't auto-upgrade)
- The full init → add → release → deploy → enable/disable → archive lifecycle

### DISC-44: No Commerce OAuth Scopes Registered on CLI Client

**The CLI's OAuth client has zero commerce scopes registered**, yet the CLI supports building, deploying, and managing commerce apps via `gddy platform app` commands.

**Registered scopes (`rust/src/scopes.rs`):**
- `apps.app-registry:read/write`
- `domains.domain:read/create`
- `domains.dns:update`
- `domains.nameserver:update`
- `hosting.paas.*` (apps, code, deploy, github, secrets, logs)
- `offline_access`

**Missing entirely:**
- `commerce.order:read/create/update/cancel/complete/archive`
- `commerce.store:read`
- `commerce.business:read/update`
- `commerce.product:read/write`
- `commerce.customer:read/create/update`
- `commerce.fulfillment:read/create/update`
- `commerce.channel:read`
- `commerce.tax:read/create/write/delete`
- `commerce.transaction:read`
- `commerce.metafield:read/create/update/delete`
- `commerce.fulfillment-plan:read`

**Scope registry file:** `rust/src/scopes.rs` — all scopes requestable via `gddy auth login -s <scope>` must be declared in the `declare_scopes!` macro. The `validate_requested_scopes()` function in `rust/src/auth.rs:141` rejects any scope not in this registry with `"unsupported OAuth scope(s) requested"`.

**Impact:** Developers can use `gddy api call` to hit commerce REST endpoints (the API catalog resolves scopes at runtime and bypasses the scope registry), but they **cannot** use `gddy auth login -s commerce.order:read` to pre-authorize commerce scopes. When the cached token lacks a required commerce scope and the CLI attempts OAuth step-up, the step-up fails because the CLI client isn't registered for commerce scopes on the authorization server. This creates a confusing experience: the CLI lets you build commerce apps but can't fully exercise the commerce APIs it helps you deploy.

### DISC-45: Environment-Specific Config Files (`godaddy.<env>.toml`) — Not Documented

**CLI behavior (`rust/src/config/mod.rs:346-351`):**
```rust
pub fn config_path(env: Option<&str>) -> std::path::PathBuf {
    match env {
        None | Some("prod") => std::path::PathBuf::from("godaddy.toml"),
        Some(e) => std::path::PathBuf::from(format!("godaddy.{e}.toml")),
    }
}
```

When using `--env test`, the CLI reads `godaddy.test.toml`, not `godaddy.toml`. Similarly, `--env ote` reads `godaddy.ote.toml`. The same pattern applies to `.env` files (`rust/src/config/mod.rs:355-359`): `--env test` reads `.env.test`.

**Docs say:** Only `godaddy.toml` is referenced anywhere in the developer docs. There is no mention of environment-specific config files or the naming convention.

**Impact:** Developers deploying to non-prod environments (test, ote) will get CONFIG_ERROR failures because the CLI silently looks for a file that doesn't exist (`godaddy.test.toml`) while the developer only has `godaddy.toml`. The workaround (copying the file) is non-obvious and undiscoverable from the docs.

### DISC-46: Webhook Subscriptions Require Unique URLs — Not Documented or Validated

**Server behavior:** The `createRelease` GraphQL mutation rejects releases where two or more webhook subscriptions share the same `url` value, returning a generic `ValidationError` with message `"Failed to create release"` and no detail about which field is invalid.

**CLI behavior:** The CLI does not validate subscription URL uniqueness in `config validate` or at release time — it passes the subscriptions through to the GraphQL API, which silently rejects them.

**Docs say:** The webhook configuration examples don't mention any uniqueness constraint on subscription URLs.

**Impact:** Developers who configure multiple webhook subscriptions pointing to the same endpoint (a common pattern — single webhook receiver handling all event types) will get a cryptic `"Failed to create release"` error with no indication that duplicate URLs are the cause. This is especially confusing because the docs show `url = "/webhooks"` as the subscription URL without any caveat about uniqueness.

### DISC-47: Platform Is GA (No Feature Flag)

_(Renumbered from DISC-44 in previous version.)_

The PROD platform module (`main:rust/src/platform/mod.rs`) has **no feature flag**:
```rust
GroupSpec::new("platform", "Build and manage GoDaddy Platform integrations")
    .with_long("...")
// No .with_feature_flag() call
```

The platform commands are Generally Available in production. The docs don't mention platform maturity or availability status.

---

## 13. Summary Table

| ID | Category | Severity | Summary |
|----|----------|----------|---------|
| DISC-01 | Flags | Medium | `init` has `--label` and `--config` flags not in docs |
| DISC-02 | Syntax | High | `enable`/`disable` examples in overview omit required `--store-id` |
| DISC-03 | Flags | Low | `deploy` short flag `-n` not in docs |
| DISC-04 | Flags | Low | `release --description` flag not in docs |
| DISC-05 | Commands | High | `archive` command entirely undocumented |
| DISC-06 | Commands | Medium | `update` command entirely undocumented (label + description only, no status) |
| DISC-07 | Commands | Medium | `list` command not documented in app guides |
| DISC-08 | Commands | Medium | `add action` command undocumented |
| DISC-09 | Commands | High | `add extension` (embed/checkout/blocks) commands undocumented |
| DISC-10 | Commands | Low | `add subscription` command not fully documented |
| DISC-11 | Commands | **Critical** | `add settings` command entirely undocumented — whole feature invisible |
| DISC-12 | Commands | **Critical** | `config validate` command entirely undocumented — offline validation invisible |
| DISC-13 | Flags | High | `--human` flag in docs doesn't exist; should be `--output human` |
| DISC-14 | Syntax | Low | Inconsistent positional vs `--name` across commands |
| DISC-15 | Syntax | Low | Positional vs flag inconsistency not explained |
| DISC-16 | Config | **Critical** | `actions`, `dependencies`, `extensions`, `settings` TOML sections undocumented |
| DISC-17 | Config | Low | Subscription URLs can be relative (proxy-relative); not clear in docs |
| DISC-18 | Config | Low | `description` required at init but optional in TOML schema |
| DISC-19 | Settings | **Critical** | `[[settings]]` config schema entirely undocumented |
| DISC-20 | Settings | **Critical** | `settings-form-v1` presentation system (7 field types, visibleWhen, sections) undocumented |
| DISC-21 | Settings | High | Release command's settings resolution/validation undocumented |
| DISC-22 | Settings | High | Embedded platform guides (containing settings docs) are undocumented |
| DISC-23 | Lifecycle | High | 5 app statuses exist but only 2-3 documented |
| DISC-24 | Lifecycle | Medium | `update` does NOT accept `--status` (explicitly rejected); status via enable/disable/archive only |
| DISC-25 | Validation | High | Docs conflate `validate` (remote) and `config validate` (local) — two separate commands |
| DISC-26 | Validation | Medium | Most config validation rules not documented (especially settings rules) |
| DISC-27 | URLs | Medium | CLI accepts HTTP at init but docs setup page says HTTPS-only |
| DISC-28 | URLs | Medium | Config validation doesn't check public-routability (init does) |
| DISC-29 | Credentials | Medium | `.env` has 4 keys; docs show 3 (missing `GODADDY_PUBLIC_KEY`) |
| DISC-30 | Credentials | Medium | `publicKey` credential returned but not explained |
| DISC-31 | Credentials | Low | Env values are JSON-quoted; docs show bare values |
| DISC-32 | Webhooks | Medium | `webhook events --human` is actually `--output human` |
| DISC-33 | Webhooks | Low | Event list truncation at 50 not documented |
| DISC-34 | Workflow | High | Release captures actions/subscriptions/extensions/settings from TOML — not just a "snapshot" |
| DISC-35 | Workflow | Low | Deploy-before-release error not documented |
| DISC-36 | Workflow | High | Settings have no release inheritance — critical gotcha undocumented |
| DISC-37 | Workflow | Medium | Existing stores don't auto-upgrade on new release — undocumented |
| DISC-38 | API | Low | GraphQL backend not documented |
| DISC-39 | Deploy | High | Security scanner (SEC101-SEC115) not documented |
| DISC-40 | Deploy | Medium | esbuild dependency not documented |
| DISC-41 | UX | Low | `next_actions` system not documented |
| DISC-42 | UX | Medium | `--output` format flag not in app docs |
| DISC-43 | Guides | High | Embedded CLI guides with unique content not referenced in docs |
| DISC-44 | Scopes | **Critical** | No commerce OAuth scopes registered on CLI client; `gddy auth login -s commerce.*` fails. Scope registry: `rust/src/scopes.rs` |
| DISC-45 | Config | High | Environment-specific config files (`godaddy.<env>.toml`) entirely undocumented; causes CONFIG_ERROR on non-prod deploys |
| DISC-46 | Webhooks | High | Webhook subscriptions require unique URLs; server rejects duplicates with cryptic error; not documented or client-validated |
| DISC-47 | Platform | Low | Platform GA status not documented |

### Priority Breakdown

- **Critical (6):** DISC-11, DISC-12, DISC-16, DISC-19, DISC-20, DISC-44
- **High (13):** DISC-02, DISC-05, DISC-09, DISC-13, DISC-21, DISC-22, DISC-23, DISC-25, DISC-34, DISC-36, DISC-39, DISC-43, DISC-45, DISC-46
- **Medium (14):** DISC-01, DISC-06, DISC-07, DISC-08, DISC-24, DISC-26, DISC-27, DISC-28, DISC-29, DISC-30, DISC-32, DISC-37, DISC-40, DISC-42
- **Low (13):** DISC-03, DISC-04, DISC-10, DISC-14, DISC-15, DISC-17, DISC-18, DISC-31, DISC-33, DISC-35, DISC-38, DISC-41, DISC-47

### Key Differences from the Previous (Incorrect) Analysis

The previous version of this document was mistakenly based on the local `wip-docs` branch, which has ~11,000 lines removed from `main`. The corrected analysis based on PROD `main` reveals:

1. **Added 5 Critical findings (DISC-11, DISC-12, DISC-16 expanded, DISC-19, DISC-20):** The entire settings system (`add settings` command, `config validate` command, `[[settings]]` TOML schema, `settings-form-v1` presentation format) exists in PROD but is completely absent from the developer docs. This was invisible in the previous analysis because the local branch had deleted all settings code.

2. **Corrected DISC-06 (update command):** The previous analysis incorrectly stated PROD has `--status ACTIVE|INACTIVE` on the `update` command. In reality, PROD explicitly rejects `--status` with a test. Only `--label` and `--description` are accepted.

3. **Added DISC-36, DISC-37:** Settings release inheritance and store upgrade gotchas documented only in embedded CLI guides.

4. **Added DISC-43, DISC-44:** The PROD platform module embeds two guides and has no feature flag (it's GA). The local branch added an experimental feature flag and deleted the guides.

5. **Expanded DISC-34:** The PROD release command is 640 lines (vs 229 on local) and performs settings resolution, presentation validation, and schemaVersion injection — significant undocumented behavior.

6. **Total findings increased from 35 to 47**, with 6 at Critical severity (up from 0).

7. **Added DISC-44 (Critical):** The CLI's OAuth client (`rust/src/scopes.rs`) has no commerce scopes registered, so `gddy auth login -s commerce.*` fails with `"unsupported OAuth scope(s) requested"`. The CLI builds and deploys commerce apps but can't exercise the commerce APIs.

8. **Added DISC-45 (High):** Environment-specific config files (`godaddy.test.toml`, `godaddy.ote.toml`) are entirely undocumented. The naming convention is in `rust/src/config/mod.rs:346-351` but nowhere in the developer docs.

9. **Added DISC-46 (High):** The app registry server requires unique URLs across webhook subscriptions within a release, but this constraint is undocumented and not client-validated — the server returns a generic `"Failed to create release"` ValidationError with no field-level detail.
