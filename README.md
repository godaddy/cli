# GoDaddy CLI

Agent-first CLI for interacting with the GoDaddy Developer Platform.

> Looking for the original, TypeScript-based `godaddy` CLI (`@godaddy/cli` on
> npm)? It's maintained on the `original` branch — including its
> `godaddy-cli` agent skill, which isn't installable from this branch.

## Installation

### macOS / Linux (and Git Bash / MSYS2 / Cygwin on Windows)

```bash
curl -fsSL https://github.com/godaddy/cli/releases/latest/download/install.sh | bash
gddy --version
```

### Windows (PowerShell)

```powershell
irm https://github.com/godaddy/cli/releases/latest/download/install.ps1 | iex
gddy --version
```

Both installers download, checksum-verify, and install the binary for your platform. If you'd rather install by hand, download the archives from the [latest release](https://github.com/godaddy/cli/releases/latest) and put the binary on your `PATH`.

Once installed, `gddy update check` / `gddy update apply` handles upgrades in place.

### Agentic Skills

To install Claude Code skills for the `gddy` CLI, run the following:

```bash
claude plugin marketplace add godaddy/cli
claude plugin install gddy@godaddy
```

For other agents, install using the [`skills`](https://github.com/anthropics/skills) CLI.

```bash
npx skills add godaddy/cli --skill gddy --agent <agent>
```

Swap `<agent>` for whichever agent you use (`claude-code`, `cursor`, `codex`, `windsurf`, `opencode`, ...). Run `npx skills add --help` for the full list of supported agents.

## Quickstart

```bash
gddy                              # environment/auth snapshot + full command tree
gddy auth login                   # opens a browser for OAuth
gddy domain available example.com # check if a domain is registerable
gddy domain list                  # list domains in your account
```

Most commands need authentication. You may be taken through an interactive login process if you are not currently logged in, if your login has expired, or if your last auth token needs additional permissions. Run `gddy auth login` to log in explicitly.

For non-interactive workflows, you can use a [Personal Access Token (PAT)](https://developer.godaddy.com/en/docs/api-users/auth) instead; store the PAT with `gddy pat add` or use it in a `GDDY_PAT` environment variable.

## What you can do

Use `gddy --help` or `gddy tree` to get a comprehensive list of available commands. Top-level commands include:

- `domain` — list your domains, check availability, get suggestions, and register new ones
- `dns` — view and edit a domain's DNS records

We're actively working to expand the CLI to cover additional GoDaddy products.

### Developer Platform

The Developer Platform command tree is currently an Experimental preview. [Enable
Experimental commands](./docs/feature-flags.md) in your environment:

```bash
export GDDY_MIN_STAGE=experimental
```

...then begin with `gddy platform app init`.

- `gddy platform app` — create, configure, release, and deploy GoDaddy Platform apps
- `gddy platform actions` — discover the action contracts an app can declare
- `gddy platform webhook` — inspect webhook event types for app subscriptions
