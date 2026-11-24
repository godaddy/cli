---
summary: What to change in your scripts and settings when you upgrade gddy across a breaking release
---

# Upgrade `gddy` across a breaking release

`gddy` follows semantic versioning. A release can break your scripts only when it changes the first version number that isn't zero. While `gddy` is at version 0, that's the second number (0.2 to 0.3). From version 1 on, it's the first number (1.4 to 2.0). Patch releases never break anything (0.2.38 to 0.2.39, for example).

Run `gddy --version` to see which version you have. After `gddy update apply` crosses one of these releases, it suggests this guide.

Each section covers one step and is named after the two versions it connects. If you skip several releases, read every section between your old version and your new one, from the top down. Each section lists what changed, who it affects, and what to change.

## 0.1 to 0.2

This release changes how you search the command list and how you point `gddy` at a different environment. Scripts that never used `--search` or an environment override aren't affected.

### Search is a command

`--search` used to be a flag that attached to any command, and it could swallow that command's output. It's now a command of its own:

```
gddy search nameserver --scope domain
```

`--scope` limits the search to one part of the command tree, for example `domain` or `domain:list`. Passing `--search` to another command now fails with `unexpected argument '--search' found`. Replace it with `gddy search`.

### Environment variables

Overrides for an environment's endpoints now use one fixed set of names that apply to the environment you're using, whatever its name. Before, the names started with the environment's name in upper case, with hyphens replaced by underscores (`OTE_ACCOUNT_URL`).

| Before | After |
|---|---|
| `<ENV>_OAUTH_AUTH_URL` | `GDDY_AUTH_URL` |
| `<ENV>_OAUTH_TOKEN_URL` | `GDDY_TOKEN_URL` |
| `<ENV>_DOMAINS_API_URL` | `GDDY_DOMAINS_API_URL` |
| `<ENV>_ACCOUNT_URL` | `GDDY_ACCOUNT_URL` |
| `<ENV>_API_URL` | Removed. Set `api_url` in `environments.toml`. |

The old names are ignored. A script that still sets them keeps running, but against the default endpoints.

### Custom environments

An environment that existed only because `<ENV>_API_URL` was set no longer resolves. `--env staging` now fails with `unknown environment "staging"` and lists the environments it knows.

Define the environment in `gddy/environments.toml` in your operating system's config directory (`~/.config/gddy/environments.toml` on Linux) instead:

```toml
[staging]
api_url = "https://api.staging.example.com"
client_id = "your-oauth-client-id"
```

Then run `gddy env set staging`, or pass `--env staging` to a single command.

### Mistakes are errors now

Settings that used to be ignored without a message now stop the command:

- A malformed override, from an environment variable or `environments.toml`, fails with an error, for example `field auth_url: "not-a-url" is not a valid http(s) URL`. It used to fall back to the default endpoint. A blank override still falls back to the default.
- An unknown name for `--env` fails with an error that lists the known environments. Commands that look up an environment's API address (`gddy api` and `gddy hosting`, for example) used to use the production address without telling you.

If a script began failing after this upgrade, run it with `--debug` and check which environment variable or `--env` value it sets.

## 0.2 to 0.3

This release changes the JSON that `gddy dns add`, `gddy dns set` and `gddy dns delete` print, and what `gddy dns add --dry-run` does. Scripts that only check the exit code, or that read `domain`, `type`, `name`, `created`, `replaced`, `unchanged`, `deleted` or `failed`, aren't affected. Scripts that read a list of records out of the output of these three commands are.

### One `records` list

`add`, `set` and `delete` now return the same `records` list in a dry run and in a real run. Each entry describes one record the command changed, or with `--dry-run` would change. It has an `action`, the record's `recordId`, and the record before (`old`) and after (`new`). `gddy guide dns` describes every field.

| Command | Before | After |
|---|---|---|
| `add` | `results` held `{ "data", "status": "created" }` for each value. It showed only with `--fields results`. | `records` has one entry per created record. Read `new.data` for the value and `recordId` for the new id. `action` and `records` are in the default output. |
| `set` | `records` listed every record at the name after the change, in the form `dns list` prints. Releases before 0.2.38 returned only the counts. | `records` lists what the command did to each record, including the ones it left alone and the ones it deleted (their `new` is `null`). Read `new.data`, `new.ttl` and `recordId`. For every record now at the name, run `gddy dns list`. |
| `set --dry-run` | `plan` held `{ "action", "recordId", "data" }`, with the new value in `data` and no old value. | `records` replaces `plan`. Read the new value from `new.data` and the value it replaces from `old.data`. |
| `delete` | Listed no records. | `records` lists each removed record, with its `recordId` and its value in `old.data`. |
| `delete --dry-run` | `records` held `{ "recordId", "data", "status" }`. `status` was `would delete` or `would fail (missing recordId)`. | `action` replaces `status` and is `delete` or `fail`. Read the value from `old.data`, and the reason for a `fail` from `error`. |

The type-specific fields of a record (`priority`, `port`, `service`, `protocol`, `weight`, `flag`, `tag` and `parameters`) used to sit beside `data` in a delete preview. Now they're inside `old` and `new`.

Scripts that read the old paths get nothing back, with no error. Change them before you upgrade:

| Before | After |
|---|---|
| `.data.results[].data` | `.data.records[].new.data` |
| `.data.plan[] \| select(.action == "replace") \| .data` | `.data.records[] \| select(.action == "replace") \| .new.data` |
| `.data.records[] \| select(.status == "would delete") \| .recordId` | `.data.records[] \| select(.action == "delete") \| .recordId` |

### When nothing changes

A `set` that changes nothing used to report the action `set`, and a `delete` that matched nothing reported `delete`. They now say `nothing to change` and `nothing to delete`, in a dry run and in a real run. A script that tests `action` for those runs needs the new values. The counts are a steadier test: `replaced`, `created` and `deleted` are all `0` when a `set` changes nothing.

### `add --dry-run` checks your command

`gddy dns add --dry-run` used to print `{ "command": "dns:add", "action": "dry-run: would execute" }` without reading anything. It passed input that the real command rejects. It now runs the same checks as a real `add` and reads the zone. You have to be logged in, and it fails for the same reasons. It returns the same `records` list as a real `add`, with `action` set to `would add`. Each `recordId` is `null`, because the API assigns the id when it writes the record.

The API refuses some values because of what's already in the zone, for example a duplicate or a CNAME conflict. The preview lists each one in `records` with `action` set to `fail` and an `error` that says why. Only a preview has `fail` entries. A real run that fails stops with an error, as before. A preview can't check the API's own rules for a value, so it doesn't promise that a real run succeeds.
