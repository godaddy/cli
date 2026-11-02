---
summary: Manage settings on domains you already own — auto-renew, transfer lock, nameservers
---

# Managing domain settings with `gddy`

This guide is for domains **you already own**. Use it to inspect a domain and change settings such as auto-renewal, transfer lock, and nameservers.

To **register a new domain**, follow `gddy guide domain-purchase` instead. To manage **DNS records** (A, CNAME, MX, and so on), follow `gddy guide dns`.

Most setting changes run **asynchronously**: the command returns an operation ID and a status. If the status is not finished yet, poll with `domain operation status`, then confirm with `domain get`.

## Inspect your domains

List domains on your account (status, expiry, and auto-renew are shown by default):

```
gddy domain list
```

Show every detail for one domain you own — including auto-renew, transfer lock, nameservers, privacy, and expiry:

```
gddy domain get example.com
```

Use `get` after a settings change to confirm the value you expect.

## Auto-renewal

When auto-renew is on, the domain renews automatically before it expires using the payment method on file. Turn it on or off for a domain you own:

```
gddy domain auto-renew set example.com --enable
gddy domain auto-renew set example.com --disable
```

Exactly one of `--enable` or `--disable` is required. Turning auto-renew off means the domain will not renew on its own — you would need to renew it before expiry if you want to keep it.

## Transfer lock

Transfer lock (registrar lock) blocks outbound transfers to another registrar while it is on. Lock or unlock a domain you own:

```
gddy domain transfer-lock set example.com --enable
gddy domain transfer-lock set example.com --disable
```

Exactly one of `--enable` or `--disable` is required. Unlock only when you intend to allow a transfer away from GoDaddy; re-enable the lock when you are done.

AI assistants: Before using `--disable` on transfer lock, confirm the end user intends to allow outbound transfer of this domain. Unlocking removes registrar-level protection against unauthorized transfers.

## Nameservers

Replace the full set of nameservers for a domain you own. Pass `--nameserver` once per host. This overwrites the existing nameservers:

```
gddy domain nameserver set example.com \
  --nameserver ns1.example.net \
  --nameserver ns2.example.net
```

Confirm with `gddy domain get example.com` after the operation finishes. Changing nameservers does not change DNS *records* at GoDaddy — use `gddy guide dns` for those.

## Checking an async operation

Setting changes return an `operationId` and a `status`. When the status is still in progress, re-check until it finishes:

```
gddy domain operation status <operation-id>
```

Then confirm the domain itself:

```
gddy domain get example.com
```

If a change fails, the operation status (or the original command error) explains why; fix the issue and try again if appropriate.

## Related guides

- `gddy guide domain-purchase` — find, quote, and register a new domain
- `gddy guide dns` — list, add, set, and delete DNS records
