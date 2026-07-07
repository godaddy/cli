# GoDaddy CLI authentication

The GoDaddy CLI supports two ways to authenticate:

- **OAuth via `gddy auth login`** — interactive, browser-based login. Best for local development.
- **Personal Access Token (PAT) via `gddy pat add`** — non-interactive, long-lived token. Best for CI/CD, scripts, and headless environments.

Both can be configured at the same time. Per environment, the CLI uses a PAT when one is available; otherwise it falls back to OAuth and opens a browser.

## Interactive OAuth

Run:

```sh
gddy auth login --env prod
```

Your browser opens to the GoDaddy login screen. After you approve the CLI, an access token is stored in your OS keychain (or a protected file fallback) and reused until it expires. If a command needs wider scopes, the CLI may re-prompt through the browser.

## Personal Access Tokens

### Creating a PAT

1. Sign in to the GoDaddy Developer Portal.
2. Open **Personal Access Tokens**.
3. Choose a name, the scopes you need, and an expiration (up to 365 days).
4. Copy the plaintext token immediately — it is shown only once.

The token looks like:

```text
gd_pat_<base62>_<8-hex-crc>
```

### Storing a PAT in the CLI

Read the token from stdin so it does not appear in shell history:

```sh
echo 'gd_pat_...' | gddy pat add --env prod "CI token"
```

Alternatively, pass it explicitly:

```sh
gddy pat add --env prod --token 'gd_pat_...' "CI token"
```

PATs are saved to your platform's `gddy` configuration directory (e.g. `~/.config/gddy/pat.toml` on Linux) with owner-only permissions where the platform supports it (`0600`).

### Listing and removing PATs

List stored PATs (only the last four characters are shown):

```sh
gddy pat list
```

Remove the PAT for an environment:

```sh
gddy pat remove --env prod
```

Removing the PAT from the CLI does **not** revoke it in the Developer Portal.

### Using PATs in CI

Instead of storing a PAT in the registry, supply it through an environment variable:

```sh
export GDDY_PAT=gd_pat_...
gddy domain list --env prod
```

For per-environment tokens:

```sh
export GDDY_PAT_PROD=gd_pat_...
export GDDY_PAT_OTE=gd_pat_...
gddy domain list --env prod
```

Per-environment variables take precedence over `GDDY_PAT`.

### How the CLI chooses a credential

For each command, the CLI resolves credentials in this order:

1. `GDDY_PAT_<ENV>` environment variable
2. `GDDY_PAT` environment variable
3. Stored PAT for the environment in the `gddy` configuration directory (e.g. `~/.config/gddy/pat.toml` on Linux)
4. OAuth token from `gddy auth login` (browser flow)

If a PAT is configured, the CLI sends it as:

```text
Authorization: Bearer gd_pat_...
```

The GoDaddy API gateway exchanges the PAT for a short-lived access token and enforces the PAT's scopes. If a command needs scopes the PAT does not grant, the API returns `403` and the CLI surfaces that error.

## Security notes

- Treat PATs like passwords. Do not commit them to source control.
- Prefer `GDDY_PAT_<ENV>` environment variables in CI over storing PATs in the registry file.
- Regenerate leaked or suspected PATs in the Developer Portal immediately.
- The CLI validates PAT format before storing, but does **not** contact the API to verify a PAT is still active.
