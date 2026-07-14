---
name: gddy
description: Use GoDaddy's beta CLI (`gddy`) to search, register, and manage domains and DNS records. Load this skill whenever a task involves running `gddy` commands, parsing their JSON output, finding or buying a domain, editing DNS records (A, CNAME, MX, TXT, etc.) for a domain hosted at GoDaddy, or reading a page under developer.godaddy.com/docs/api-users/**. Also trigger any time Claude is about to fetch a GoDaddy developer-docs page, even mid-task, since it changes how that fetch should be done. `gddy` is a separate tool from GoDaddy's older `godaddy` CLI (applications, deployments, webhooks) — do not use this skill for that tool, and don't trigger for other registrars or DNS providers (Cloudflare, Route 53, Namecheap, etc.) unless GoDaddy is explicitly involved.
---

# gddy — GoDaddy domains & DNS CLI

`gddy` is GoDaddy's beta CLI for domain search, registration, and DNS management. It's a separate, independently-installed tool from GoDaddy's older `godaddy` CLI (applications, deployments, webhooks) — different binary, different install method, different auth and config. Installing or using one doesn't affect the other. For application, deployment, or webhook tasks, use the `godaddy-cli` skill instead of this one.

Both tools exist side by side because `gddy` is the newer of the two and under active development, with more surface area on its roadmap than what's documented here — it already has early, not-yet-stable support for things like applications and webhooks alongside its domain/DNS focus. Stick to the domain/DNS commands documented in this skill unless you've confirmed a newer capability is ready via `gddy --help`.

It's under active development, and moves faster than its own docs and README — see "When the CLI and the docs disagree" below.

## Fetching GoDaddy developer docs: curl the llms.mdx mirror, don't WebFetch the HTML

Read this before fetching anything under `developer.godaddy.com/docs/**`.

WebFetch always pipes the page through a summarizing model, no matter what the prompt says. For documentation with exact command syntax, flag names, and code samples, that summarization silently drops or rewrites details — a flag gets dropped, a code sample gets paraphrased into something that no longer runs. GoDaddy publishes a full-text markdown mirror of every docs page specifically so agents can get the real content instead. Use it.

To read any docs page, curl its markdown mirror instead of fetching the HTML:

```bash
# HTML page:   https://developer.godaddy.com/docs/api-users/cli-setup
# curl this:   https://developer.godaddy.com/llms.mdx/api-users/cli-setup
curl -fsSL https://developer.godaddy.com/llms.mdx/api-users/cli-setup
```

The rule for deriving the URL: take the path after `/docs/` (or `/en/docs/`), and fetch `https://developer.godaddy.com/llms.mdx/<that-path>`. This applies even if the user hands you the HTML URL directly — rewrite it yourself before fetching. If you only need the table of contents for a section, `https://developer.godaddy.com/llms.mdx/api-users` (no subpage) gives the full page tree for that section.

Do this for every GoDaddy docs page you touch, not just ones the user explicitly names — if a task leads you to open another page on the same docs site partway through, curl its mirror too rather than falling back to WebFetch.

## When the CLI and the docs disagree, trust the CLI

`gddy`'s own `--help` output reflects the actual installed version; docs and the README can lag behind a release, since it's beta and moves fast. Before relying on a remembered or documented flag shape, especially for anything destructive or paid, run `gddy <command> --help` and trust that over any doc page or README. This matters most for domain purchase, which is a two-step flow that some docs oversimplify (see below).

## Setup

Install:

```bash
# macOS / Linux / Git Bash / MSYS2 / Cygwin
curl -fsSL https://github.com/godaddy/cli/releases/latest/download/install.sh | bash

# PowerShell
irm https://github.com/godaddy/cli/releases/latest/download/install.ps1 | iex
```

Verify with `gddy --version`. This installs the `gddy` binary alongside any existing `godaddy` CLI — the two are separate tools.

Authenticate with `gddy auth login` (opens a browser for OAuth). Check state with `gddy auth status`, sign out with `gddy auth logout`. For non-interactive use (CI, scripts), use a Personal Access Token instead — manage one with `gddy pat add/list/remove`, or set the `GDDY_PAT`/`GDDY_PAT_<ENV>` env var directly (checked before OAuth). Note PATs can't do everything: domain purchase specifically requires a customer-scoped OAuth login (see below).

Every command accepts `--env <ote|prod>` to pick environment (default `prod`) and `--debug` for verbose output.

## Domain purchase: it's a two-step quote-then-purchase flow, not one command

This is the highest-stakes command in the CLI — it charges money and can't be undone — so get the shape right. Some docs and even the README show it as a single `gddy domain purchase <domain> --agree --confirm` call; that's a simplification and doesn't match what the CLI actually does. The real flow locks in registration terms first, then executes against that locked quote:

1. **Quote** — decide the registration period, privacy, and nameservers here; they get baked into the quote token:
   ```bash
   gddy domain quote example.com --period 1 --privacy
   ```
   Returns a single-use `quoteToken` valid for roughly 10 minutes, cached locally alongside the contacts file.

2. **Purchase** — spend the quote token:
   ```bash
   gddy domain purchase --quote-token <token> --agree --confirm
   ```
   `--agree` means "I consent to this quote's required legal agreements" — leave it off once first if you want to see what agreements apply before agreeing. `--confirm` is a separate, explicit acknowledgment that this charges the account. Both are required; there's no lower-friction path, by design. Purchase needs an OAuth login (`gddy auth login`) — a bare PAT is rejected here even though PATs work fine for read commands.

3. Purchase is async. The command waits briefly and reports status; if it's still pending, check back with `gddy domain operation status <operation-id>`. A completed purchase shows up in `gddy domain list`.

Before quoting, contact info needs to exist: `gddy domain contacts init` writes a starter `contacts.toml` template (registrant/admin/billing/tech, all commented out) to the OS config directory. Any role left commented out falls back to the account default; any role you fill in needs all of its required fields (name, email, phone, full address).

Always confirm with the user before running `purchase` — it's real money, and the action is not reversible.

## Domain search and lookup

```bash
gddy domain suggest "coffee shop" --tlds com --tlds net --length-min 4 --length-max 15
gddy domain available example.com --check-type fast   # or: full (live registry check, slower)
gddy domain list --status ACTIVE
gddy domain get example.com
gddy domain agreements --tld com --privacy
```

Global `--limit <N>` caps result counts on list-like commands.

## DNS management

Valid record types: `A AAAA ALIAS CAA CNAME MX NS SOA SRV TXT`. `NS` and `SOA` are GoDaddy-managed and read-only — you can list them but not add/set/delete them.

```bash
gddy dns list example.com --type A
gddy dns add example.com --type A --name www --data 192.0.2.1 --ttl 3600
gddy dns set example.com --type TXT --name @ --data "v=spf1 -all"
gddy dns delete example.com --type A --name www
```

`add` appends a new record; `set` replaces every record matching that type+name; `delete` removes every record matching that type+name. Type-specific requirements: `MX`/`SRV` need `--priority`; `SRV` also needs `--port`, `--weight`, `--protocol`, `--service`; `CAA` requires `--tag` (`--flag` is optional).

`set` and `delete` are destructive — support `--dry-run` to preview the change and `--reason <text>` for an audit trail. Run with `--dry-run` first and show the user what would change before applying it for real.

## Payments

`gddy payments add` opens the browser to the account's payment-methods page — no card data is ever handled by the CLI itself. Purchases fail (403/422) without a valid payment method or Good-as-Gold balance on the account; check the error's `code` field, not just the HTTP status, to tell that apart from other failures.

## Reference material

Read these only when the task needs the detail — they're not needed for common domain/DNS operations:

- `reference/api.md` — raw GoDaddy Domains API endpoints (when `gddy` isn't installed/available, e.g. CI in a non-shell language): availability/suggestions, the registration-quote → registration flow with idempotency keys, pagination, and the MCP server.
- `reference/errors-and-limits.md` — error envelope shape, retry/idempotency rules per HTTP method, and rate-limit headers/handling.
