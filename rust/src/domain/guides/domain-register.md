---
summary: Interactive domain registration wizard — guided step-by-step domain purchase
---

# Interactive domain registration with `gddy domain register`

The `register` command is an interactive wizard that walks you through
the entire domain registration process in a single session:

```
gddy domain register
```

## How it works

The wizard guides you through 5 steps:

1. **Discovery** — Search for a domain or enter one directly. If taken, view
   suggestions and pick an alternative.
2. **Options** — Choose registration period (1–10 years), WHOIS privacy, and
   auto-renewal. Optionally set custom nameservers.
3. **Contacts** — Use your account default contacts, load saved contacts from
   `contacts.toml`, or enter new ones interactively.
4. **Review & Confirm** — See the full order summary (price, renewal, agreements)
   and explicitly consent before any charge is made.
5. **Register** — Submit the registration and wait for the registry to confirm.

You can go back to a previous step at any point. Pressing Ctrl+C at any time
cancels the wizard — **no charges are made until you explicitly confirm in
Step 4**.

## Non-interactive mode

For scripts and CI, pass all options as flags:

```
gddy domain register example.com \
  --period 2 \
  --privacy true \
  --auto-renew true \
  --agree \
  --confirm
```

Required flags in non-interactive mode:
- Domain name (positional argument)
- `--agree` — consent to legal agreements
- `--confirm` — authorize the purchase

## Contacts

The wizard checks for saved contacts at `~/.config/gddy/contacts.toml`.
If found, you can reuse them without re-entering details each time.

To create a starter contacts file:
```
gddy domain contacts init
```

When you enter contacts manually during the wizard, you'll be offered to save
them for future registrations.

## Payment

A valid payment method (credit card or Good-as-Gold balance) must be on file.
If the quote fails with a payment error, the wizard will offer to open the
GoDaddy payment methods page in your browser.

## Entry from other commands

When running interactively, related commands offer to continue into registration:

- `gddy domain available example.com` — if available, asks "Would you like to register?"
- `gddy domain suggest "keywords"` — after results, asks "Would you like to register one?"
- `gddy domain quote example.com` — after pricing, asks "Would you like to purchase now?"

## Examples

```
# Full interactive wizard
gddy domain register

# Start with a specific domain (skips discovery search)
gddy domain register example.com

# Non-interactive for scripts
gddy domain register example.com --period 1 --agree --confirm

# With custom nameservers
gddy domain register example.com \
  --nameserver ns1.example.net \
  --nameserver ns2.example.net \
  --agree --confirm
```
