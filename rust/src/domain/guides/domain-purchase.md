---
summary: Register a domain with gddy — quote, review, then purchase
---

# Buying a domain with `gddy`

Registering a domain is a **two-step, quote-then-purchase** flow:

1. `gddy domain quote <domain>` locks a price and returns the legal agreements
   and a single-use **quote token** (valid ~10 minutes).
2. `gddy domain purchase --quote-token <token> --agree --confirm` accepts that
   quote and registers the domain.

A purchase is **paid and not reversible**, so `purchase` has two gates
(`--agree`, `--confirm`) on top of the token. Because the token locks the price
and settings you reviewed, you're charged exactly what the quote showed.

Registration completes **asynchronously**: `purchase` submits the registration
and waits briefly for the registry, reporting the operation's `status`
(`COMPLETED` when done). The domain then appears in `gddy domain list`. If
`purchase` gives up waiting before reaching a terminal status, it still
succeeded — check on it later with `gddy domain operation status
<operation-id>` (the ID it printed).

## The flow

1. **Find a name** (optional) — if you don't have one in mind, get suggestions
   from a seed word or phrase:
   ```
   gddy domain suggest "avocado jewelry"
   ```
   Narrow with `--tlds com --tlds dev` (repeatable), bound the name length with
   `--length-min`/`--length-max`, and cap the count with the global `--limit`.
   Suggestions are available-by-contract, but availability is re-checked at
   quote time.
2. **Check availability** (optional) — confirm a name and see its price:
   `gddy domain available example.com`
3. **Quote it** — this is where you choose the registration settings and review
   the terms and price:
   ```
   gddy domain quote example.com
   ```
   The output shows the price, the required legal agreements, the resolved
   contact/preference settings, and the `quoteToken` (with its `expiresAt`). Set
   registration options here (they're locked into the token):
   - `--period <1-10>` — registration length in years (default 1).
   - `--privacy` — add privacy protection.
   - `--no-renew` — disable auto-renew (on by default).
   - `--nameserver <host>` — custom nameserver (repeatable); omit for GoDaddy's.
4. **Purchase**, accepting the agreements and confirming the charge:
   ```
   gddy domain purchase --quote-token <token> --agree --confirm
   ```

The quote is cached locally (next to your `contacts.toml`), so **run `quote` and
`purchase` on the same machine**, within the token's ~10-minute lifetime. If the
token has expired or isn't found, just re-run `gddy domain quote`.

### The three decisions

- **The quote token** selects *which* quote you're buying — the exact domain,
  settings, and locked price you reviewed.
- `--agree` is your **consent** to the quote's legal agreements. Run `purchase`
  without it and it lists the agreements you must accept.
- `--confirm` acknowledges that the purchase **charges your account**. Without
  it, the command stops before buying.
- `--agreed-by <ip>` — the originating IP recorded with your consent. Defaults
  to `127.0.0.1`; pass your real public IP if you need accurate consent
  attribution.

## Contacts

A registration has four contact roles: **registrant**, **admin**, **billing**,
and **tech**. If you don't supply them, the API uses your GoDaddy account's
default contacts — which is usually what you want. Contacts are part of the
quote, so configure them *before* you quote.

If you register many domains and want to set your own contacts once, save them
in a `contacts.toml` in your `gddy` config directory. The quickest start is to
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
`organization`, `address2`.
