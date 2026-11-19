---
summary: How to register an application-settings form via godaddy.toml (settings-form-v1)
---

# Application settings

An application-settings capability lets a GoDaddy Platform Application (GPA) contribute a form to a Commerce-owned settings surface (for example `tax-center`). Merchants fill it out, and the GPA's own `load`, `save`, and `validate` endpoints own the data. `app-registry-api` stores and validates only the registration and presentation metadata. It never sees merchant values. This guide covers the `gddy platform app` side of registering one. For the platform contract itself (lifecycle endpoints, signing, `settings-api` composition), go to `app-registry-api`'s `docs/GPA-SETTINGS-REGISTRATION.md` and `docs/SETTINGS.md`.

## Add a settings form to an application

To add a settings form to an application, add the placement, add the form, release the application, and enable it for a store.

1. Add the placement, which writes the group, slug, entryPath, order, capabilities, and icon fields into `godaddy.toml` (go to [Placement options](#placement-options) for the options):

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

2. Add the form, either as a `[settings.presentation]` block in the entry `add settings` just wrote, or as a JSON file (go to [The form](#the-form) for the details).

3. Release the application (go to [Release errors](#release-errors) for what can fail):

   ```sh
   gddy platform app release --application-id <id> --version <version>
   ```

4. Enable the application for a store, then confirm it with `enablement` (go to [Enable and backfill](#enable-and-backfill) for how releases and stores interact):

   ```sh
   gddy platform app enable <name> --store-id <storeId>
   gddy platform app enablement --store-id <storeId>
   ```

### Placement options

These options need a note:

- `entryPath` is relative to the application's registered `proxy_url`, same as action URLs.
- `--capability` defaults to `read` and `write` server-side if omitted.
- `--icon-name` and `--icon-library` must be given together or not at all.
- A `config` capability requires `read` plus one or more `--config-key` values; the command writes those values to `metadata.configKeys`.

### The form

`add settings` only writes placement metadata. The actual field and section definitions (`presentation`) aren't flag-driven. `presentation` and `presentationFile` are mutually exclusive. To use a JSON file, point to it with `--presentation-file <path>`, or edit a `presentationFile` entry into the entry afterward. Go to [Presentation shape](#presentation-shape) for the details.

### Release errors

`release` resends every setting in `godaddy.toml`, the same as it does for actions, subscriptions, and extensions. It rejects any settings entry with neither `presentation` nor `presentationFile`. The error has the code `VALIDATION_ERROR`. It says the setting has no presentation, and that you need to add a `[settings.presentation]` block or a `presentationFile` before releasing.

### Enable and backfill

`enable` makes the placement discoverable for a store, and `enablement` lists the apps enabled on that store. Settings, like actions, subscriptions, and uiExtensions, are keyed per release with no inheritance. A store already enabled against an older release doesn't pick up settings added in a newer one until you re-run `enable` for that store.

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

A field `key` becomes a property in the merchant's saved `values` document and must stay stable once merchants have data. Don't rename a field key without a GPA-owned migration. Section keys are display-only and safe to change freely.

Supported field types (`type` discriminates the shape; every field needs `type`, `key`, and `label`):

- **`text`** and **`textarea`**: string value. Optional: `placeholder`, `minLength`, `maxLength`, `defaultValue` (string).
- **`number`**: numeric value. Optional: `min`, `max`, `step`, `suffix`, `defaultValue` (number).
- **`boolean`**: true or false value. Optional: `defaultValue` (bool).
- **`select`**: one value chosen from `options` (required, non-empty). Each option: `value`, `label`, optional `description`. Add more by repeating the `[[...options]]` array-of-tables header, one block per option, at whatever nesting depth the field sits (for example `[[settings.presentation.sections.fields.item.fields.options]]` inside a `list-group` item field). Optional: `defaultValue` must match one option's `value`.
- **`multi-select`**: array of values chosen from `options` (required, non-empty). Optional: `minItems`, `maxItems`, `defaultValue` (array, each entry must match an option).
- **`list-group`**: array of objects, one merchant-added item per array entry. `item.idField` names the item's reserved stable-UUID property (renderer-generated, and the GPA echoes it back unchanged, so it can't also be an editable field key). `item.titleField` (optional) names a `text`, `textarea`, `number`, or `select` field used to label each item in the UI. `item.fields` is a list of fields using the same types as the other bullets. A `list-group` may nest one level further (max depth 2), but not deeper.

Every field, section, and `list-group` item field also accepts `description`. Sections also accept `visibleWhen = { field = "...", equals = ... }` to show or hide based on another top-level field's value.

Worked example: the full `godaddy.toml` shape for a manual-tax-style GPA with a nested `list-group`:

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

### Reference a JSON file instead of inline TOML

For a GPA that already keeps its presentation as a JSON fixture, point `presentationFile` at it instead of re-authoring the same form as TOML:

```toml
[[settings]]
group = "tax-center"
slug = "manual-tax"
entryPath = "/settings/manual-tax"
presentationFile = "fixtures/manual-tax-registry-presentation.json"
```

The referenced file must be the complete API presentation object: `type` (`"form"`), `schemaVersion` (`"settings-form-v1"`), and `sections`. This is the same shape `createRelease.settings[].presentation` expects, so you can reuse an existing fixture verbatim. The path is relative to the directory containing the `godaddy.toml` being released, not the shell's working directory. `presentation` and `presentationFile` are mutually exclusive. Both forms run through the same field and section validation and produce an identical release payload. The file is only opened at `release`. Like inline `presentation`, it's optional at `add settings` and `config validate` time. A missing, unreadable, or malformed file, or one with the wrong `type` or `schemaVersion`, fails the release with a `VALIDATION_ERROR` naming the resolved path.

### Link presentation (`settings-link-v1`)

For a GPA that must own its own configuration page or provider authorization flow instead of a native form, `presentation` can instead be a link:

```toml
[[settings]]
group = "payment-methods"
slug = "paypal-payments"
entryPath = "/settings/paypal"
capabilities = ["read", "open"]

[settings.presentation]
label = "Configure PayPal"
openMode = "new-window"
```

A link presentation requires the `read` and `open` capabilities and can also declare `delete` and `config`. Form-only capabilities (`write`, `validate`, and `test`) and duplicates are rejected. `open` is rejected on a form presentation. `label` must be non-empty. `openMode` only accepts `"new-window"` for now. `--presentation-file` also accepts a link's full API object (`type: "link"`, `schemaVersion: "settings-link-v1"`, `label`, `openMode`). Go to `app-registry-api`'s `docs/SETTINGS.md` for the full lifecycle contract this registers into. App Registry never stores a merchant-specific launch URL. It publishes the placement and lets Settings API request one through the GPA's `open` route when a merchant invokes it.

### Public config (`gpa-settings-config-v1`)

A form or link contribution may declare `config` when the GPA serves `POST {entryPath}/config`. The registration must also declare `read` and an allowlist of 1 to 16 public response keys. Keys must be unique and match `^[a-z][A-Za-z0-9]{0,63}$`.

```sh
gddy platform app add settings \
  --group payment-methods \
  --slug paypal-payments \
  --entry-path /settings/paypal \
  --capability read --capability open --capability config --capability delete \
  --config-key clientId --config-key merchantId --config-key disableFunding
```

The command writes:

```toml
[[settings]]
group = "payment-methods"
slug = "paypal-payments"
entryPath = "/settings/paypal"
capabilities = ["read", "open", "config", "delete"]

[settings.metadata]
configKeys = ["clientId", "merchantId", "disableFunding"]
```

App Registry stores only this allowlist. Settings API strips response properties outside it before returning public config to consumers. `metadata.configKeys` without the `config` capability is rejected.

## What the CLI validates locally and what the server validates

`gddy platform app add settings` and `release` catch cheap, structural problems before any network call:

- `group` and `slug` match the platform's slug pattern (`lowercase-with-dashes`).
- `entryPath` is a route-safe path (`/`-prefixed, no query string, fragment, or `..`) and doesn't overlap another setting's `entryPath` in the same manifest.
- `capabilities` are a subset of `read`, `write`, `validate`, `test`, `delete`, `open`, `config`. `open` is only valid on a link presentation. Link presentations are limited to required `read` and `open` plus optional `config` or `delete`.
- `config` requires `read` and 1-16 valid, unique `metadata.configKeys`.
- `icon.library` is one of `ux`, `lucide`, `commerce`.
- Every field and section `key` matches the platform's key pattern, `select` and `multi-select` have at least one option, and no two fields or sections share a key.
- A link presentation's `label` is non-empty and `openMode` is `"new-window"`.
- `presentation` and `presentationFile` aren't both set on the same entry.

These checks run as soon as the manifest is touched, not just at release.

Deeper semantics stay server-validated: bounds consistency (`maxLength ≥ minLength`), a `defaultValue` actually matching a registered option or satisfying bounds, and `list-group` nesting depth. A rejection there surfaces as a `release` API error, not a local one.

## Gotchas

- **No release inheritance.** `release` resends every current setting from `godaddy.toml`; leaving one out doesn't archive it globally, but any store enabled against the *new* release loses it.
- **Existing stores don't auto-upgrade.** Adding settings to a release only affects stores enabled *after* that release goes active. Re-run `gddy platform app enable <name> --store-id <storeId>` per store to backfill, then `gddy platform app enablement --store-id <storeId>` to confirm.
- **`presentation` or `presentationFile` is mandatory before release, not before `add settings`.** A placement-only entry parses and works fine for every other command (`add action`, `info`, `validate`, `deploy`). It only fails at `release`, with the error described earlier.
