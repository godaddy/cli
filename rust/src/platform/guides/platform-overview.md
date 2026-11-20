---
summary: How to build, configure, and ship a GoDaddy Platform Application (GPA) end to end
---

# Build a GoDaddy Platform Application with `gddy platform app`

`gddy platform app` registers and manages a GoDaddy developer-platform application (a "GPA"), described by a `godaddy.toml` manifest in your working directory. This guide walks the full lifecycle in order. Every command also has its own `--help` with the same detail.

## Create the application

```sh
gddy platform app init --name my-app --url https://example.com --proxy-url https://example.com/proxy --scopes commerce.order:read,commerce.order:update
```

This calls the app-registry API to create the application, then writes `godaddy.toml` (and a per-env secrets file) to the current directory. `url` and `proxy-url` must be publicly resolvable HTTP(S). Localhost, loopback, and private IPs are rejected. Re-run with `--config <path>` to seed flags from an existing manifest instead of retyping them. Covered by the default login; a PAT needs the `apps.app-registry:read` and `apps.app-registry:write` scopes.

To allow additional OAuth callbacks, add `redirect_uris` to the manifest passed with `--config`:

```toml
url = "https://my-app.example.com"

# Additional OAuth callbacks only. The defaults derived from `url` are automatic.
redirect_uris = [
  "https://auth.example.net/oauth/callback",
  "https://staging.example.net/oauth/callback",
]
```

The list accepts up to five unique HTTPS URLs, each no longer than 2048 characters. Credentials and fragments aren't allowed, and the list must not repeat `url` or the root-relative `/api/godaddy/callback` resolved against `url`. Omitting `redirect_uris` leaves the existing App Registry allowlist unchanged during an update; `redirect_uris = []` clears the additional callbacks. `init --config` sends the list when creating an application. Both `update` (including a label-only or description-only update) and `deploy` synchronize it when the key is present, so an explicit empty list clears remote extras in either flow. The list contains extras only and doesn't replace the callbacks registered automatically from `url`.

## Configure the application

Most `gddy platform app add <subcommand>` commands only append to `godaddy.toml`:

- `add action --name <name> --url <url>`: an HTTP endpoint the platform calls on the app's behalf.
- `add subscription --name <name> --url <url> --events <event...>`: a webhook route for platform events; run `gddy platform webhook event` to see valid event types.
- `add extension <embed|checkout|blocks> ...`: a UI extension bundle (run that subcommand with `--help` for details).
- `add settings --group <group> --slug <slug> --entry-path <path> ...`: placement metadata for a merchant-facing settings form or link. This only writes placement fields (group, slug, entryPath, order, capabilities, and icon). The presentation itself (`[settings.presentation]`) has to be hand-authored in `godaddy.toml` afterward. Go to the `platform-settings` guide (`gddy guide platform-settings`) for the full presentation shape.

Run `gddy platform app config validate` any time to check the manifest without a network call. It checks every rule the API would otherwise enforce (required fields, URL, UUID, and semver shapes, and settings placement rules). It reports every violation it finds, not just the first.

## Release the application

```sh
gddy platform app release --application-id <id> --version 1.2.3
```

This resends every action, subscription, UI extension, and settings entry in `godaddy.toml` as one versioned release. Omitting an entry from the manifest doesn't archive it globally, but a store enabled against a *newer* release won't have it. A settings entry with no `[settings.presentation]` block fails the release with a `VALIDATION_ERROR`. A manifest that fails to parse or validate also fails the release outright. Only a genuinely missing manifest falls back to an empty release. The version must be semver.

## Deploy the extensions

```sh
gddy platform app deploy
```

Bundles, security-scans, and uploads the extensions declared in `godaddy.toml`, streaming progress as JSON events.

## Enable or disable per store

```sh
gddy platform app enable <name> --store-id <storeId>
gddy platform app disable <name> --store-id <storeId>
gddy platform app enablement --store-id <storeId>
```

`enable` and `disable` make the application available on one store, or remove it from that store. That includes everything in its latest release: actions, subscriptions, extensions, and settings. Settings have no inheritance across releases. A store already enabled against an older release doesn't pick up settings added by a newer one until you re-run `enable` for that store.

`enablement` is the read counterpart: it lists which applications are enabled on that store (an empty list if none). Use it to verify an `enable` or audit what's bound to a store. Default output fields are `name`, `status`, and `releaseVersion` (the enabled release’s version string, not the full release object). Pass `--fields id,label` or `--fields all` to include the application id, the label, or both. App listing (`list` and `info`) is separate. Those commands show developer apps in App Registry, not per-store enablements.

## Other useful commands

- `gddy platform app validate <name>`: check *remote* application state (`url` and `proxy-url` set, not INACTIVE), as opposed to `config validate`'s local manifest check.
- `gddy platform app info --name <name>` or `list`: inspect a single app or list all of them (developer catalog, not store enablements).
- `gddy platform app enablement --store-id <storeId>`: list apps enabled on a store (defaults: name, status, releaseVersion).
- `gddy platform app archive <name>`: irreversible; confirm the name with `list` first.
- `gddy platform action` and `gddy platform webhook`: browse the platform's action and webhook-event catalogs (used when choosing values for `add action` and `add subscription`).

## Related

- `gddy guide platform-settings`: the `settings-form-v1` and `settings-link-v1` presentation shapes in depth.
