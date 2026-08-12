---
summary: Manage DNS records with gddy — list, add, set, and delete
---

# Managing DNS with `gddy`

`gddy dns` manages a domain's DNS records with the following commands:

| Command | Destructive? | Purpose |
|---|---|---|
| `dns list` | No | List records, optionally filtered by type/name |
| `dns add` | No | Append new records |
| `dns set` | Yes | Replace every record for a type+name |
| `dns delete` | Yes | Remove every record for a type+name |

`NS` and `SOA` records are managed by GoDaddy and read-only — you can list them, but not add, set, or delete them.

## Listing records

```
gddy dns list example.com
gddy dns list example.com --type A
gddy dns list example.com --type A --name www
```

Without filters, `list` returns every record. `--type` narrows to one record type; `--name` further narrows to one name and requires `--type`. 

## Adding records

`add` appends new records without touching anything that already exists:

```
gddy dns add example.com --type A --name www --data 192.0.2.1 --ttl 3600
```

Pass `--data` more than once to add several records for the same type+name in one call — each becomes a separate record, and a failure on one doesn't stop the rest (the result reports created/failed counts with a per-value breakdown). `--ttl` defaults to 3600 seconds when omitted.

A name can hold a CNAME or other record types, never both. Adding into a name that already has the other kind fails with an error naming the conflict — see `set --replace-conflicting-types` below if you actually want to replace it.

## Replacing records (`set`, destructive)

`set` replaces every record matching a type+name pair with exactly the `--data` value(s) you give it. It reconciles rather than deleting and recreating everything: it reuses existing record IDs, deletes whatever's no longer in your list, and creates whatever's new.

```
gddy dns set example.com --type TXT --name @ --data "v=spf1 -all"
```

Preview the plan before writing:

```
gddy dns set example.com --type TXT --name @ --data "v=spf1 -all" --dry-run
```

The dry-run preview lists the planned action (replace/create/delete) per record so you can confirm it before it runs for real. `set` is not atomic — a partial failure reports how many records were replaced, created, and deleted before it happened.

To also remove a conflicting CNAME (or, when setting a CNAME, any other type) at that name, add `--replace-conflicting-types`.

## Deleting records

`delete` removes every record matching a type+name pair:

```
gddy dns delete example.com --type A --name www
```

Deleting something that doesn't match is a no-op success (`deleted: 0`), not an error. Preview with `--dry-run` first for anything you're not fully sure about.

## Type-specific flags

Most of these only apply to certain record types — set them only when the type calls for it:

| Flag | Applies to | Notes |
|---|---|---|
| `--priority` | `MX`, `SRV` | |
| `--port`, `--weight`, `--protocol`, `--service` | `SRV` | |
| `--flag`, `--tag` | `CAA` | `--tag` is required for CAA (e.g. `issue`, `issuewild`, `iodef`); `--data` carries the CA domain/value |
