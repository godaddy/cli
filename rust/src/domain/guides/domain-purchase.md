---
summary: Quote, review, and purchase a domain with gddy
---

# Buy a domain with `gddy`

Registering a domain is a **two-step, quote-then-purchase** flow:

1. `gddy domain quote <domain>` locks a price and returns the legal agreements
   and a single-use **quote token** (valid ~10 minutes).
2. `gddy domain purchase --quote-token <token> --agree --confirm` accepts that
   quote and registers the domain.

A purchase is **paid and not reversible**, so `purchase` has two gates (`--agree`, `--confirm`) on top of the token. The quote token locks the registration price and settings you reviewed.

**The quoted price doesn't include taxes and fees.** The ICANN fee and any applicable taxes are added when you register, so the total charged is higher than the price shown by `suggest`, `available` and `quote`. Whether tax applies depends on the billing address of your payment method. The `Premium Fees` column is something else: it lists the extra cost of a premium domain and is empty for a non-premium domain. Once the registration completes, `purchase` gives you the order ID, and `gddy shopping order get <order-id>` shows the total charged.

Registration completes **asynchronously**: `purchase` submits the registration and waits briefly for the registry, reporting the operation's `status` (`COMPLETED` when done). The domain then shows up in `gddy domain list`. If `purchase` gives up waiting before reaching a terminal status, it will give you an operation ID which you can use to check its progress with:
`gddy domain operation status <operation-id>`.

## Quote and purchase a domain

To buy a domain, run up to four commands in order. The first two are optional. Quoting a domain gives you a quote token, which locks in the price and the registration settings you review.

1. Get name suggestions (optional), from a seed word or phrase or with no phrase (go to [Find a name](#find-a-name) for the options):

   ```
   gddy domain suggest "avocado jewelry"
   gddy domain suggest
   ```

2. Check availability (optional) to confirm a name and see its price:

   ```
   gddy domain available example.com
   ```

3. Quote the domain to review the terms and price and lock in your registration settings (go to [Quote options](#quote-options) for the settings):

   ```
   gddy domain quote example.com
   ```

4. Read the quote, which shows the price, the required legal agreements, the resolved contact and preference settings, and the `quoteToken` (with its `expiresAt`).

5. Purchase the domain, accepting the agreements and confirming the charge (go to [Quote lifetime](#quote-lifetime) for how long the quote lasts):

   ```
   gddy domain purchase --quote-token <token> --agree --confirm
   ```

### Find a name

Give `suggest` a seed word or phrase, or leave the phrase out to get suggestions based on what we know about you and your account. These options narrow the results:

- `--tlds com --tlds dev`: limit the suggestions to these top-level domains (repeatable).
- `--length-min` and `--length-max`: bound the name length.
- `--limit`: cap the count (a global option).

Suggestions are meant to be available, but availability is checked again when you quote.

### Quote options

Set registration options when you quote, because they're locked into the quote token:

- `--period <1-10>`: registration length in years (default 1).
- `--privacy`: add privacy protection.
- `--no-renew`: disable auto-renew (on by default).
- `--nameserver <host>`: custom nameserver (repeatable); omit for GoDaddy's.

### Quote lifetime

The quote is cached locally (next to your `contacts.toml`), so **run `quote` and `purchase` on the same machine**, within the token's ~10-minute lifetime. If the token has expired or isn't found, just re-run `gddy domain quote`.

### The three decisions

- **The quote token** selects *which* quote you're buying: the exact domain,
  settings, and registration price you reviewed.
- `--agree` is your **consent** to the quote's legal agreements. Run `purchase`
  without it and it lists the agreements you must accept.
- `--confirm` acknowledges that the purchase **charges your account**. Without
  it, the command stops before buying.

AI assistants: Before using `--agree` or `--confirm`, show the end user the quoted price from `domain quote` (including any premium fees), tell them that taxes and fees such as the ICANN fee are added on top so the total charged will be higher, and show each required legal agreement's title and URL (from `quote`'s `requiredAgreements`), and obtain their explicit confirmation to proceed. The user must be able to click through and read the agreements. Do not infer confirmation from a general request to buy or register a domain.

## Buy a domain with other products

To buy a domain together with other GoDaddy products, or to pay with the same checkout session as them, use `gddy shopping checkout create --domain <domain>` instead. That path uses your account's default registration settings. Go to `gddy guide shopping` for the details.

## International domain names

You can type a name either as Unicode (`münchen.de`) or in its encoded form (`xn--mnchen-3ya.de`); both reach the same domain. Output shows Unicode by default. Add `--punycode` to any `domain` command that shows names to see the encoded form instead, or make that your default with `gddy config set punycode true` (undo it with `gddy config unset punycode`). Precedence is the `--punycode` flag, then the `GDDY_PUNYCODE` environment variable, then your saved setting, then Unicode, so `--punycode=false` turns the encoded form back off for one command.

## Contacts

A registration has four contact roles: **registrant**, **admin**, **billing**, and **tech**. If you don't supply them, the API uses your GoDaddy account's default contacts, which is usually what you want. Contacts are part of the quote, so configure them *before* you quote.

If you register many domains and want to set your own contacts once, save them in a `contacts.toml` in your `gddy` config directory. The quickest start is to generate a template and edit it:

```
gddy domain contact init
```

That writes a starter `contacts.toml` (with every role commented out) to:

- Linux: `~/.config/gddy/contacts.toml`
- macOS: `~/Library/Application Support/gddy/contacts.toml`
- Windows: `%APPDATA%\gddy\contacts.toml`

Open it, uncomment the role(s) you want, and fill in the values. Each role is an optional table. A role you omit falls back to the account default. A role you uncomment must be **complete**, with every required field from the example that follows, or the file fails to load.

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

Required fields per role: `name_first`, `name_last`, `email`, `phone`, `address1`, `city`, `state`, `postal_code`, `country`.
Optional fields: `organization`, `address2`.
