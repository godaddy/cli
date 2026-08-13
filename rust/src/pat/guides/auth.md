---
summary: How authentication and authorization works with gddy
---

# GoDaddy CLI authentication

The GoDaddy CLI supports two ways to authenticate:

- **OAuth via `gddy auth login`** — interactive, browser-based login.
- **Personal Access Token (PAT) via `gddy pat add`** — non-interactive, long-lived token. Best for CI/CD, scripts, and headless environments.

Both can be configured at the same time. Per environment, the CLI prefers a PAT when one is available; otherwise it falls back to OAuth (see [How the CLI chooses a credential](#how-the-cli-chooses-a-credential) for the full precedence order).

Each command that requires authentication also has a required set of permissions, called scopes.

## Interactive OAuth

If a command requires authentication or permissions that have not yet been granted, `gddy` will open the browser to a GoDaddy login screen and will ask you for confirmation that you want to grant `gddy` permissions. If you do not yet have a GoDaddy account, you can register within the login screen.

Once you have logged in, an access token is stored in your OS keychain (or a protected file fallback), and it is reused until it expires. If a command needs wider scopes, the CLI may re-prompt through the browser to grant those additional permissions.

If you want to authenticate in advance, you can explicitly run:

```sh
gddy auth login
```

...and you can ask for specific scopes with repeatable `--scope x` arguments. To view a list of all available scopes, run `gddy auth scopes`.

If you want to check your current authentication status, run `gddy auth status`.

## Personal Access Tokens

Personal Access Tokens, or PATs, allow you to assign permissions to the CLI in cases where an interactive browser experience is not possible.

### Creating a PAT

1. Sign in to the [Personal Access Token page](https://developer.godaddy.com/personal-access-token).
2. Click **+ Generate Token**.
3. In the **Generate personal access token** dialog, fill in a **Name**, an **Expiration** (in days), and the **Scopes** the token needs (see the [PAT scopes reference](https://developer.godaddy.com/en/docs/api-users/auth#pat-scopes) — e.g. `domains.domain:read`, `domains.dns:update`). A write-scoped token also satisfies reads for the same resource; a read-scoped token is refused on writes.
4. Click **Generate Token**. The token is shown once in a "Copy your new token" dialog — copy it immediately. You can't retrieve it again from the Personal Access Token page; if you lose it, revoke it and generate a new one.

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

Removing the PAT from the CLI does **not** revoke it in the Developer Portal. To revoke it for real, go to the [Personal Access Token page](https://developer.godaddy.com/personal-access-token), click the trash icon next to the token, and confirm.

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

### Security notes

- Treat PATs like passwords. Do not commit them to source control.
- Prefer `GDDY_PAT_<ENV>` environment variables in CI over storing PATs in the registry file.
- Regenerate leaked PATs on the [Personal Access Token page](https://developer.godaddy.com/personal-access-token) immediately.
- The CLI only validates PAT format before storing, but it does **not** contact the API to verify the PAT is still active or has the scopes you need.
