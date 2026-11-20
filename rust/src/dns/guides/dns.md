---
summary: Manage DNS records with gddy by listing, adding, setting, deleting, exporting, and importing them
---

# Manage DNS with `gddy`

`gddy dns` manages a domain's DNS records with the following commands:

| Command | Destructive? | Purpose |
|---|---|---|
| `dns list` | No | List records, optionally filtered by type and name |
| `dns add` | No | Append new records |
| `dns set` | Yes | Replace every record for a type+name |
| `dns delete` | Yes | Remove every record for a type+name |
| `dns export` | No | Write all records as a zone file or JSON |
| `dns import` | Yes | Add records from a zone file or JSON; with `--replace`, make the domain match it |

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

## Export a domain's records

`export` writes every record of a domain as a zone file. That is the master file format from [RFC 1035](https://www.rfc-editor.org/rfc/rfc1035), also called BIND format, which DNS servers and most DNS providers read:

```
gddy dns export example.com > example.com.zone
```

Use it to copy a domain's records to another domain or provider, to keep them in version control, or to take a backup before a large change. The file lists the records for the domain itself first, then the rest by name. It leaves out the SOA record and the NS records for the domain itself, because every DNS provider creates its own, and `import` skips them too. NS records for a subdomain, which hand it to other name servers, are kept. It also leaves out the `A` record of a parked domain and non-standard `ALIAS` records. Add `--format json` if you prefer that format.

If a zone ever holds a record a standard DNS provider can't use, `export` stops and names it. Add `--standard-only` to leave those records out instead.

## Import records

`import` reads records from a file, or from standard input with `--file -`, and adds them to a domain:

```
gddy dns import example.com --file example.com.zone
gddy dns export example.com | gddy dns import other-example.com --file -
```

The input is a zone file in the same RFC 1035 format, like the ones `export` writes or another provider gives you, or the JSON that `dns list -o json` and `export --format json` write. `import` works out which from the content; use `--format zone` or `--format json` to say. In a zone file, names are relative to the domain you import into. To import a file written for `example.com` into `other-example.com`, change its `$ORIGIN` line.

By default an import only adds. The import leaves a record the domain already has alone, gives a record that matches except for its TTL the new TTL, and deletes nothing. A name that already holds a value can hold more, so importing an MX record with a different mail server leaves both in place.

Every record is checked before anything is written. If any record is wrong, for example a TTL outside 600 to 86400 seconds, a missing priority or a name outside the domain, the import stops and writes nothing. It lists each problem with its line. Records of one type and name must share a TTL, because they are written as a set.

Preview any import first. A preview shows each record as create, replace or unchanged, and any that would fail:

```
gddy dns import example.com --file example.com.zone --dry-run
```

### Make a domain match the file

Add `--replace` to make the domain hold exactly the records in the input. This deletes the records the input doesn't have, so preview it first:

```
gddy dns import example.com --file example.com.zone --replace --dry-run
```

An import isn't atomic. If a record fails, the others are still written and the error lists each outcome. Running the same import again is safe, because records already in place are left alone.

### What isn't imported

`NS` and `SOA` records are managed by GoDaddy, so `import` skips them and says how many. It skips a name server record for a subdomain too, which hands that subdomain to other servers. The import warns about each one, because that subdomain won't be delegated.

A zone file can't use `$INCLUDE` or `$GENERATE`, and records can't use the generic `TYPE123` or `\#` forms. Write those records out in full.

## Errors

A failed command returns JSON with a `error.code` property set to one of the following values:

| Code | Meaning |
|---|---|
| `RECORD_EXISTS` | `add` or `set` hit a record that is already there, or a name that can't hold both (a CNAME and other types) |
| `ZONE_NOT_FOUND` | the domain has no DNS zone for your account; check the name with `gddy domain list` |
| `RECORD_NOT_FOUND` | the record addressed isn't there |
| `VALIDATION_ERROR` | a flag or value was rejected, such as a flag the record type doesn't use (`--priority` on a `TXT` record), or a record in an `import` file that isn't valid |

`add`, `set`, `delete` and `import` attempt every record and list each outcome in the message. When all the failures share one of the codes above, the error carries it; when they differ, it is `UNEXPECTED_ERROR` and the message says which failed and why.

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

## Records with different values

Every flag above except `--ttl` can be given once, to apply to every `--data` value, or once per `--data` value, to pair with them in order. This is how one command writes records that differ, such as mail servers at three priorities:

```
gddy dns set example.com \
  --type MX \
  --name @ \
  --data aspmx.l.mail-example.com --priority 1 \
  --data alt1.aspmx.l.mail-example.com --priority 5 \
  --data alt2.aspmx.l.mail-example.com --priority 10
```

`set` makes the name hold exactly those records, so an old MX record that isn't listed is removed. Any other count is an error: `--priority` given twice for three `--data` values doesn't say which record is left over. Pairing is by order, not by position on the command line, so check the plan with `--dry-run` before a destructive `set`.

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
