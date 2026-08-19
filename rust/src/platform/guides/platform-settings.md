---
summary: How to register an application-settings form via godaddy.toml (settings-form-v1)
---

# Application settings

An application-settings capability lets a GoDaddy Platform Application (GPA) contribute a form to a Commerce-owned settings surface (e.g. `tax-center`) — merchants fill it out, the GPA's own `load`/`save`/`validate` endpoints own the data. `app-registry-api` stores and validates only the registration and presentation metadata; it never sees merchant values. This covers the `gddy platform app` side of registering one — for the platform contract itself (lifecycle endpoints, signing, `settings-api` composition), see `app-registry-api`'s `docs/GPA-SETTINGS-REGISTRATION.md` and `docs/SETTINGS.md`.

## Workflow

1. **Add the placement** — `gddy platform app add settings` writes the group/slug/entryPath/order/capabilities/icon fields into `godaddy.toml`:

   ```sh
   gddy platform app add settings \
     --group tax-center \
     --slug godaddy-tax \
     --title "GoDaddy Tax" \
     --description "Choose manual tax rules or automatic U.S. ZIP-code tax rates." \
     --entry-path /settings/godaddy-tax \
     --order 10 \
     --capability read --capability write --capability validate \
     --icon-name percent --icon-library lucide
   ```

   `entryPath` is relative to the application's registered `proxy_url`, same as action URLs. `--capability` defaults to `read`+`write` server-side if omitted. `--icon-name`/`--icon-library` must be given together or not at all.

2. **Hand-add the form** — this command only writes placement metadata; the actual field/section definitions (`presentation`) aren't flag-driven — write a `[settings.presentation]` block directly into the entry `add settings` just wrote. See "Presentation shape" below.

3. **Release** — `gddy platform app release --application-id <id> --version <version>` resends every setting in `godaddy.toml`, same as it does for actions/subscriptions/extensions. It rejects any settings entry with no `presentation` (`{"error":{"code":"VALIDATION_ERROR","message":"settings 'godaddy-tax' has no presentation — add a [settings.presentation] block before releasing"}}`).

4. **Enable/backfill** — `gddy platform app enable <name> --store-id <storeId>` makes the placement discoverable for a store. Settings (like actions/subscriptions/uiExtensions) are keyed per-release with no inheritance — a store already enabled against an older release doesn't pick up settings added in a newer one until `enable` is re-run for that store.

## Presentation shape

`presentation` is a `settings-form-v1` form: one or more sections, each with one or more fields. Add it as nested tables under the setting's own `[[settings]]` entry:

```toml
[[settings.presentation.sections]]
key = "defaults"
label = "Calculation defaults"

[[settings.presentation.sections.fields]]
type = "select"
key = "calculateUsing"
label = "Calculate using"
required = true
defaultValue = "destination"

[[settings.presentation.sections.fields.options]]
label = "Customer destination"
value = "destination"

[[settings.presentation.sections.fields.options]]
label = "Order origin"
value = "origin"
```

A field `key` becomes a property in the merchant's saved `values` document and must stay stable once merchants have data — don't rename a field key without a GPA-owned migration. Section keys are display-only and safe to change freely.

Supported field types (`type` discriminates the shape — every field needs `type`, `key`, `label`):

- **`text`** / **`textarea`** — string value. Optional: `placeholder`, `minLength`, `maxLength`, `defaultValue` (string).
- **`number`** — numeric value. Optional: `min`, `max`, `step`, `suffix`, `defaultValue` (number).
- **`boolean`** — true/false value. Optional: `defaultValue` (bool).
- **`select`** — one value chosen from `options` (required, non-empty). Each option: `value`, `label`, optional `description`. Add more by repeating the `[[...options]]` array-of-tables header — one block per option, at whatever nesting depth the field sits (e.g. `[[settings.presentation.sections.fields.item.fields.options]]` inside a `list-group` item field). Optional: `defaultValue` must match one option's `value`.
- **`multi-select`** — array of values chosen from `options` (required, non-empty). Optional: `minItems`, `maxItems`, `defaultValue` (array, each entry must match an option).
- **`list-group`** — array of objects, one merchant-added item per array entry. `item.idField` names the item's reserved stable-UUID property (renderer-generated, GPA echoes it back unchanged — it can't also be an editable field key). `item.titleField` (optional) names a `text`/`textarea`/`number`/`select` field used to label each item in the UI. `item.fields` is a list of fields using the same types above — `list-group` may nest one level further (max depth 2), but not deeper.

Every field, section, and `list-group` item field also accepts `description`. Sections additionally accept `visibleWhen = { field = "...", equals = ... }` to show/hide based on another top-level field's value.

Worked example — the full `godaddy.toml` shape for a manual-tax-style GPA with a nested `list-group`:

```toml
[[settings]]
group = "tax-center"
slug = "manual-tax"
title = "Manual Tax"
entryPath = "/settings/manual-tax"
order = 10
capabilities = ["read", "write", "validate"]

[settings.icon]
name = "percent"
library = "lucide"

[[settings.presentation.sections]]
key = "rules"
label = "Tax rules"

[[settings.presentation.sections.fields]]
type = "list-group"
key = "rules"
label = "Rules"
minItems = 1

[settings.presentation.sections.fields.item]
idField = "id"
titleField = "displayName"

[[settings.presentation.sections.fields.item.fields]]
type = "select"
key = "country"
label = "Country"

[[settings.presentation.sections.fields.item.fields.options]]
label = "United States"
value = "US"

[[settings.presentation.sections.fields.item.fields]]
type = "text"
key = "displayName"
label = "Display at checkout"
```

## What the CLI validates locally vs. server-side

`gddy platform app add settings`/`release` catch cheap, structural problems before any network call: `group`/`slug` match the platform's slug pattern (`lowercase-with-dashes`); `entryPath` is a route-safe path (`/`-prefixed, no query string/fragment/`..`) and doesn't overlap another setting's `entryPath` in the same manifest; `capabilities` are a subset of `read`, `write`, `validate`, `test`, `delete`; `icon.library` is one of `ux`, `lucide`, `commerce`; every field/section `key` matches the platform's key pattern, `select`/`multi-select` have at least one option, and no two fields/sections share a key.

Deeper semantics stay server-validated — bounds consistency (`maxLength ≥ minLength`), a `defaultValue` actually matching a registered option or satisfying bounds, and `list-group` nesting depth. A rejection there surfaces as a `release` API error, not a local one.

## Gotchas

- **No release inheritance.** `release` resends every current setting from `godaddy.toml`; leaving one out doesn't archive it globally, but any store enabled against the *new* release loses it.
- **Existing stores don't auto-upgrade.** Adding settings to a release only affects stores enabled *after* that release goes active — re-run `gddy platform app enable <name> --store-id <storeId>` per store to backfill.
- **`presentation` is mandatory before release, not before `add settings`.** A placement-only entry parses and works fine for every other command (`add action`, `info`, `validate`, `deploy`) — it only fails at `release`, with the message shown above.
