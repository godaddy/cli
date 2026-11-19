---
summary: Manage DNS records with gddy by listing, adding, setting, and deleting them
---

# Manage DNS with `gddy`

`gddy dns` manages a domain's DNS records with the following commands:

| Command | Destructive? | Purpose |
|---|---|---|
| `dns list` | No | List records, optionally filtered by type and name |
| `dns add` | No | Append new records |
| `dns set` | Yes | Replace every record for a type+name |
| `dns delete` | Yes | Remove every record for a type+name |

`NS` and `SOA` records are managed by GoDaddy and read-only. You can list them, but not add, set, or delete them.

## List records

```
gddy dns list example.com
gddy dns list example.com --type A
gddy dns list example.com --type A --name www
```

Without filters, `list` returns every record. `--type` narrows to one record type; `--name` further narrows to one name and requires `--type`. 

## Add records

`add` appends new records without touching anything that already exists:

```
gddy dns add example.com --type A --name www --data 192.0.2.1 --ttl 3600
```

Pass `--data` more than once to add several records for the same type+name in one call. Each becomes a separate record. `--ttl` is in seconds, from 600 (10 minutes) to 86400 (24 hours), and defaults to 3600 when omitted.

A name can hold a CNAME or other record types, never both. Adding into a name that already has the other kind fails with an error naming the conflict. To replace it instead, use `set --replace-conflicting-types`, covered in the `set` section.

Add `--dry-run` to the command to validate the command and preview the changes without actually applying them.

## Replace records (`set`, destructive)

`set` replaces every record matching a type+name pair with exactly the `--data` value(s) you give it.

```
gddy dns set example.com --type TXT --name @ --data "v=spf1 -all"
```

Preview the plan before writing:

```
gddy dns set example.com --type TXT --name @ --data "v=spf1 -all" --dry-run
```


Add `--dry-run` to the command to validate the command and preview the changes without actually applying them.

For `SRV` and `TLSA` records, give `add` and `set` the record's own name and the service and protocol (or port and protocol) as separate flags, for example `--name chat --service sip --protocol tcp`.

## Delete records

`delete` removes every record matching a type+name pair:

```
gddy dns delete example.com --type A --name www
```

Deleting something that doesn't match is a no-op success (`deleted: 0`, action `nothing to delete`), not an error. Preview with `--dry-run` first for anything you're not fully sure about.

For `SRV` and `TLSA` records, the name doesn't include the service or port. `--type SRV --name chat` matches every SRV record listed under `chat`, whatever its service, so run `list` or `--dry-run` first to see which will be removed. To target one service, give the full name instead, for example `--name _sip._tcp.chat` (for `TLSA`, `_443._tcp.www`).

## International domain names

Domain names and record names can be typed either as Unicode (`münchen.de`) or in their encoded form (`xn--mnchen-3ya.de`); both reach the same domain. Output shows Unicode by default. Add `--punycode` to any DNS command to show the encoded form instead, or make that your default with `gddy config set punycode true` (undo it with `gddy config unset punycode`). Precedence is the `--punycode` flag, then the `GDDY_PUNYCODE` environment variable, then your saved setting, then Unicode, so `--punycode=false` turns the encoded form back off for one command. `gddy config list` shows the current value. Record values (the `--data` of a CNAME, MX or TXT record) are never converted; enter them exactly as they should be stored.

## Type-specific flags

Some record types need extra flags:

| Record type | Required flags | Optional |
|---|---|---|
| `MX` | `--priority` | |
| `SRV` | `--priority`, `--weight`, `--port`, `--service`, `--protocol` | |
| `HTTPS`, `SVCB` | `--priority` (`0` means AliasMode) | `--parameters` |
| `TLSA` | `--usage`, `--selector`, `--matching-type`, `--port`, `--protocol` | |
| `CAA` | `--tag` (`issue`, `issuewild` or `iodef`); `--data` carries the value for that tag, for example a CA domain | `--flag` (defaults to `0`) |

`--service` and `--protocol` take the plain name, for example `--service sip --protocol tcp`. GoDaddy stores them with a leading underscore (`_sip._tcp`), which `gddy` adds for you; `--service _sip` works too. Any name made of letters, digits, `-` and `_` (up to 62 characters before the underscore) is accepted, so custom services work. Listings show the underscore form, since that is the DNS name.

Example SRV record:

```
gddy dns add example.com \
  --type SRV \
  --name chat \
  --data chat-server.example.com. \
  --priority 10 \
  --weight 5 \
  --port 5222 \
  --service xmpp \
  --protocol tcp
```
