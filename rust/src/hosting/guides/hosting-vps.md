---
summary: List your virtual private servers with their status, see one in detail, and start, stop or restart it
---

# `gddy hosting`: virtual private servers

A virtual private server (VPS) is a server you rent and control. Unlike a hosted app, it isn't created with `--app-type` and `hosting app list` doesn't show it. The `hosting vps` commands work on servers you already own.

## Concepts

The following terms and concepts are relevant to working with servers:

**Server ID**: the identifier of one server, shown as `id` by `hosting vps list`. It has the form `VPS-<datacenter>-<uuid>`, for example `VPS-phx3-3f2a9c4e-1111-4a5b-8c6d-000000000001`. Copy it exactly as shown; every server command takes it as `--server-id`. The CLI doesn't remember a server for you.

**Data center**: where the server runs: `phx3`, `ams3` or `sin2`. One list shows servers from every data center, and the server ID already carries it.

**Operation**: a start, stop or restart runs in the background. The command waits and prints the final state. Add `--no-wait` to return at once with an operation ID.

## List your servers

```sh
gddy hosting vps list
```

The command returns every server with its `name`, `id`, `status`, `os`, `controlPanel`, `createdAt` and, when the plan has one, `expiresAt`. Add `--limit <n>` to cap how many are shown, and `--output json` for the same fields as JSON.

The list doesn't show a server's IP address. Use `get` for that.

## Show one server

```sh
gddy hosting vps get --server-id VPS-phx3-3f2a9c4e-1111-4a5b-8c6d-000000000001
```

The command shows the name, status, data center, IP address, operating system and plan. `status` is the platform's own value, for example `ACTIVE` (running) or `STOPPED`.

## Start, stop or restart

```sh
gddy hosting vps start   --server-id <server-id>
gddy hosting vps stop    --server-id <server-id> --confirm
gddy hosting vps restart --server-id <server-id> --confirm
```

Stopping takes the server, and everything running on it, offline until you start it again. Restarting takes it offline for a short time. Both need `--confirm`; without it the command sends nothing. `start` needs no confirmation.

The command checks the operation every 5 seconds and gives up after 15 minutes. If the operation fails, it prints the reason and exits with an error. To check an operation later:

```sh
gddy hosting vps operation get --server-id <server-id> --operation-id <operation-id>
```

## See also

- `gddy guide hosting`: what the hosting products share.
- `gddy guide auth`: how authentication and credential storage work.
