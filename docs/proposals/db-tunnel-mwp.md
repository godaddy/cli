# Proposal: `gddy db tunnel --product mhwp` — MySQL access for Managed WordPress

Status: draft. The CLI side is implemented. The server side is not live yet.

This document covers only what the CLI does differently for Managed WordPress.
The WebSocket protocol, the local listener, and bind safety are the same as for
Node.js Hosting; see [db-tunnel.md](./db-tunnel.md). The server-side design is
internal to GoDaddy.

## Motivation

`gddy db tunnel` gives a developer a local MySQL port that relays to an app's
database over a WebSocket. For Node.js Hosting apps, the far end is the app's
long-running agent. Managed WordPress apps have no agent, so the platform
starts a short-lived **relay** for each tunnel session instead. The relay speaks
the same WebSocket protocol as the agent, so the CLI's relay code is shared.

## Usage

```bash
gddy db tunnel --app-id <app-id> --product mhwp [--port 3306]
mysql --ssl-mode=REQUIRED -h 127.0.0.1 -P 3306 -u <user> -p
```

`--product` is required: `nodejs` for Node.js Hosting, `mhwp` for Managed
WordPress (case-insensitive, the same values as `--app-type` on other hosting
commands). It only selects the mint endpoint. Everything else follows from the
mint response.

## What the CLI does

1. **Mint.** `POST {api base}/v1/airo/hosting/apps/{appId}/database-tunnel`,
   with an OAuth token that has the deploy-execute scope and
   `hosting.database.tunnel:execute`. The response is never logged, because it
   holds the relay token.
2. **Read the response.**

   | Field | Use |
   | --- | --- |
   | `url` | The relay's base URL. The CLI opens `wss://…/apps/{appId}/database/tunnel` on it. |
   | `token` | Sent as `Authorization: Bearer <token>` on every WebSocket. |
   | `pollUrl` | A readiness probe on the relay's own host. It must be HTTPS and on the same host as `url`. |
   | `expiresAt` | When the session ends. |
   | `replaced` | `true` when this mint closed the app's previous tunnel. The CLI warns. |

3. **Wait.** The relay is started for this session, so the CLI polls
   `pollUrl` until it answers `2xx`, for up to 3 minutes.
4. **Listen and relay.** From here on, the behaviour is the same as for
   Node.js: one WebSocket per local TCP connection, raw MySQL bytes in binary
   frames. The CLI prints the same `mysql` hint.

No database login is handed out, as for Node.js. The customer logs in with a
login they already have, normally WordPress's own database user from
`wp-config.php`.

## Limitations

- **Owner only.** The app's owner can open a tunnel. Collaborators cannot yet.
- **One tunnel per app.** A new `gddy db tunnel` run for the same app closes the
  previous one, and the CLI says so.
- **One hour per session.** After that, new connections are refused. Run the
  command again.
- **Clients must use TLS.** Connections from clients that do not ask for TLS,
  such as `--ssl-mode=DISABLED`, are closed. `--ssl-mode=REQUIRED` encrypts, but
  `VERIFY_IDENTITY` cannot be used against `127.0.0.1`. MariaDB's client does
  not accept `--ssl-mode`; use its `--ssl` option.
- **Idle MySQL sessions are closed by the server** after its `wait_timeout`. The
  `mysql` client reconnects on the next query. This is not a tunnel fault.
- **Packets are limited by the server's `max_allowed_packet`**, which is below
  the tunnel's 32 MiB frame limit.
- **Cold start on every run**, while the relay starts.
- **`publish` only.** The CLI has no `--variant` option yet.

## Testing

- `rust/src/db/tunnel.rs` covers the product flag, the mint response shapes
  (with and without `replaced`), and the `pollUrl` checks.
- `rust/src/hosting/client_tests.rs` covers the request path, the Bearer
  header, and the response.
- End to end: not run yet.
