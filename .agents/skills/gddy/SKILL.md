---
name: gddy
description: Use GoDaddy's beta CLI (`gddy`) to search, register, and manage domains and DNS records. Load this skill whenever a task involves running `gddy` commands, parsing their JSON output, finding or buying a domain, or editing DNS records (A, CNAME, MX, TXT, etc.) for a domain hosted at GoDaddy. `gddy` is a separate tool from GoDaddy's older `godaddy` CLI (applications, deployments, webhooks) — do not use this skill for that tool, and don't trigger for other registrars or DNS providers (Cloudflare, Route 53, Namecheap, etc.) unless GoDaddy is explicitly involved.
---

# gddy — GoDaddy domains & DNS CLI

`gddy` is GoDaddy's beta CLI for domain search, registration, and DNS management. It's separate from GoDaddy's older `godaddy` CLI, which covers applications, deployments, and webhooks.

Both tools exist side by side because `gddy` is the newer of the two, under active development, and ships new features frequently. Run `gddy --help` against your installed release for the authoritative, current command list.

## Setup

Install:

```bash
# macOS / Linux / Git Bash / MSYS2 / Cygwin
curl -fsSL https://github.com/godaddy/cli/releases/latest/download/install.sh | bash

# PowerShell
irm https://github.com/godaddy/cli/releases/latest/download/install.ps1 | iex
```

Verify that installation works by running `gddy --version`.

Authenticate with `gddy auth login` (opens a browser for OAuth). Check state with `gddy auth status`, sign out with `gddy auth logout`. For non-interactive use (CI, scripts), use a Personal Access Token instead — manage one with `gddy pat add/list/remove`, or set the `GDDY_PAT`/`GDDY_PAT_<ENV>` env var directly (checked before OAuth). Note PATs can't do everything: domain purchase specifically requires a customer-scoped OAuth login (see `gddy guide domain-purchase`).

Every command accepts `--env <ote|prod>` to pick environment (default `prod`) and `--debug` for verbose output.

## Documentation

The `gddy` CLI is self-documenting. Use the following to get more information about its capabilities:

- The `--help` flag will give you detailed documentation on any command.
- `gddy --search <keywords>` will help you find a command.
- The `gddy tree` command will give you a full listing of all commands.
- The `gddy guide` command lets you view detailed documentation that explains concepts and outlines complex workflows.

## Domain purchase

Domain purchase is a multi-step workflow. Run `gddy guide domain-purchase` for a detailed walkthrough. Always confirm with the user before finalizing a purchase — it's real money, and the action is not reversible.

## DNS management

Valid record types: `A AAAA ALIAS CAA CNAME MX NS SOA SRV TXT`. `NS` and `SOA` are GoDaddy-managed and read-only — you can list them but not add/set/delete them.

```bash
gddy dns list example.com --type A
gddy dns add example.com --type A --name www --data 192.0.2.1 --ttl 3600
gddy dns set example.com --type TXT --name @ --data "v=spf1 -all"
gddy dns delete example.com --type A --name www
```

`add` appends a new record; `set` replaces every record matching that type+name; `delete` removes every record matching that type+name. Type-specific requirements: `MX`/`SRV` need `--priority`; `SRV` also needs `--port`, `--weight`, `--protocol`, `--service`; `CAA` requires `--tag` (`--flag` is optional).

`set` and `delete` are destructive. Run with `--dry-run` first and show the user what would change before applying it for real.

## Payments

`gddy payments add` opens the browser to the account's payment-methods page — no card data is ever handled by the CLI itself. Purchases fail (403/422) without a valid payment method or sufficient account balance; check the error's `code` field, not just the HTTP status, to tell that apart from other failures.

## Reference material

Read these only when the task needs the detail — they're not needed for common domain/DNS operations:

- `reference/api.md` — raw GoDaddy Domains API fallback (when `gddy` isn't installed/available, e.g. CI in a non-shell language): base URL/auth, the public OpenAPI specs to fetch and search directly, and the durable gotchas (idempotency keys on registration, the MCP server's read-only/no-auth scope).
- `reference/errors-and-limits.md` — error envelope shape, retry/idempotency rules per HTTP method, and rate-limit headers/handling.
