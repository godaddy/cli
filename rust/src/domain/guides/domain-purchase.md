---
summary: Register a domain with gddy, including consent and saved default contacts
---

# Buying a domain with `gddy`

`gddy domain purchase <domain>` registers a domain through the GoDaddy Domains
API. A purchase is **paid and not reversible**, so the command has two gates and
records your consent to the registry's legal agreements.

Registration completes **asynchronously**: a successful command reports
`status: submitted`, and the domain appears in `gddy domain list` once the
registry finishes processing.

## The flow

1. **Check availability** (optional):
   `gddy domain available example.com`
2. **Review the legal agreements** for the TLD:
   `gddy domain agreements --tld com`
   (add `--privacy` if you will request privacy protection).
3. **Purchase**, agreeing to those agreements and confirming the charge:
   `gddy domain purchase example.com --agree --confirm`

### The two gates

- `--agree` is your **consent** to the TLD's legal agreements. Run the command
  without it once and it lists the agreements you must accept. The keys of those
  agreements are recorded with your purchase.
- `--confirm` acknowledges that the purchase **charges your account**. Without
  it, the command stops before buying. Use `--dry-run` to preview without
  charging.

### Other options

- `--period <1-10>` — registration length in years (default 1).
- `--privacy` — add privacy protection.
- `--no-renew` — disable auto-renew (on by default).
- `--nameserver <host>` — set a custom nameserver (repeatable); omit to use
  GoDaddy's nameservers.
- `--agreed-by <ip>` — the originating IP recorded with your consent. Defaults to
  `127.0.0.1`; pass your real public IP if you need accurate consent attribution.

## Contacts

A registration has four contact roles: **registrant**, **admin**, **billing**,
and **tech**. If you don't supply them, the API uses your GoDaddy account's
default contacts — which is usually what you want.

If you register many domains and want to set your own contacts once, save them in
a `contacts.toml` in your `gddy` config directory. The quickest start is to
generate a template and edit it:

```
gddy domain contacts init
```

That writes a starter `contacts.toml` (with every role commented out) to:

- Linux: `~/.config/gddy/contacts.toml`
- macOS: `~/Library/Application Support/gddy/contacts.toml`
- Windows: `%APPDATA%\gddy\contacts.toml`

Open it, uncomment the role(s) you want, and fill in the values. Each role is an
optional table: a role you omit falls back to the account default; a role you
uncomment must be **complete** (the required fields below), or the file fails to
load.

```toml
[registrant]
name_first   = "Ada"
name_last    = "Lovelace"
email        = "ada@example.com"
phone        = "+1.4805551212"
organization = "Analytical Engines"   # optional
address1     = "1 Engine Way"
address2     = "Suite 2"               # optional
city         = "Tempe"
state        = "AZ"
postal_code  = "85281"
country      = "US"                    # two-letter ISO code

[admin]
name_first  = "Ada"
name_last   = "Lovelace"
email       = "ada@example.com"
phone       = "+1.4805551212"
address1    = "1 Engine Way"
city        = "Tempe"
state       = "AZ"
postal_code = "85281"
country     = "US"

# [billing] and [tech] follow the same shape. Omit a role to use the account
# default for it.
```

Required fields per role: `name_first`, `name_last`, `email`, `phone`,
`address1`, `city`, `state`, `postal_code`, `country`. Optional fields:
`name_middle`, `organization`, `job_title`, `fax`, `address2`.
