---
summary: How to build, configure, and ship a GoDaddy Platform Application (GPA) end to end
---

# gddy platform app — building a GoDaddy Platform Application

`gddy platform app` registers and manages a GoDaddy developer-platform application (a "GPA"), described by a `godaddy.toml` manifest in your working directory. This guide walks the full lifecycle in order. Every command also has its own `--help` with the same detail.

## 1. Create the application

```sh
gddy platform app init --name my-app --url https://example.com --proxy-url https://example.com/proxy --scopes commerce.order:read,commerce.order:write
```

This calls the app-registry API to create the application, then writes `godaddy.toml` (and a per-env secrets file) to the current directory. `url`/`proxy-url` must be publicly resolvable HTTP(S) — localhost, loopback, and private IPs are rejected. Re-run with `--config <path>` to seed flags from an existing manifest instead of retyping them. Requires the `applications.*:read`/`write` scopes (`--scope` on `gddy auth login`, or a PAT with the same scopes).

## 2. Configure it locally

`gddy platform app add <subcommand>` appends to `godaddy.toml` without any network call:

- `add action --name <name> --url <url>` — an HTTP endpoint the platform calls on the app's behalf.
- `add subscription --name <name> --url <url> --events <event...>` — a webhook route for platform events; run `gddy platform webhook events` to see valid event types.
- `add extension <embed|checkout|blocks> ...` — a UI extension bundle (see that subcommand's own `--help`).
- `add settings --group <group> --slug <slug> --entry-path <path> ...` — placement metadata for a merchant-facing settings form. This only writes placement fields (group/slug/entryPath/order/capabilities/icon); the form itself (`[settings.presentation]`) has to be hand-authored in `godaddy.toml` afterward. See the `platform-settings` guide (`gddy guide platform-settings`) for the full presentation shape.

Run `gddy platform app config validate` any time to check the manifest against every rule the API would otherwise enforce (required fields, URL/UUID/semver shapes, settings placement rules) without a network call — it reports every violation found, not just the first.

## 3. Release

```sh
gddy platform app release --application-id <id> --version 1.2.3
```

Resends every action, subscription, UI extension, and settings entry currently in `godaddy.toml` as one versioned release — omitting an entry from the manifest doesn't archive it globally, but a store enabled against a *newer* release won't have it. A settings entry with no `[settings.presentation]` block fails the release with a `VALIDATION_ERROR`; a manifest that fails to parse or validate fails the release outright (only a genuinely missing manifest falls back to an empty release). Version must be semver.

## 4. Deploy

```sh
gddy platform app deploy
```

Bundles, security-scans, and uploads the extensions declared in `godaddy.toml`, then activates the latest release and its parent application. An application that is already active skips the application lifecycle mutation. Progress is streamed as JSON events.

## 5. Enable / disable per store

```sh
gddy platform app enable <name> --store-id <storeId>
gddy platform app disable <name> --store-id <storeId>
```

Makes the application (and everything in its latest release — actions, subscriptions, extensions, settings) available on, or removes it from, one store. Settings have no inheritance across releases: a store already enabled against an older release does not pick up settings added by a newer one until `enable` is re-run for that store.

## Other useful commands

- `gddy platform app validate <name>` — check *remote* application state (URL/proxy-url set, not INACTIVE), as opposed to `config validate`'s local manifest check.
- `gddy platform app info --name <name>` / `list` — inspect a single app or list all of them.
- `gddy platform app archive <name>` — irreversible; confirm the name with `list` first.
- `gddy platform actions` / `gddy platform webhook` — browse the platform's action and webhook-event catalogs (used when choosing values for `add action`/`add subscription`).

## See also

- `gddy guide platform-settings` — the `settings-form-v1` presentation shape in depth.
- `docs/application-settings.md` in this repo — the same settings content, plus a pointer to `app-registry-api`'s platform-contract docs for the Commerce-side settings surface.
