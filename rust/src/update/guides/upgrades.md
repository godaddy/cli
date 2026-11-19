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
