# GoDaddy CLI

Agent-first CLI for interacting with GoDaddy Developer Platform.

## Installation

```bash
npm install -g @godaddy/cli
godaddy --help
```

## Output Contract

All executable commands emit JSON envelopes:

```json
{"ok":true,"command":"godaddy env get","result":{"environment":"ote"},"next_actions":[...]}
```

```json
{"ok":false,"command":"godaddy application info demo","error":{"message":"Application 'demo' not found","code":"NOT_FOUND"},"fix":"Use discovery commands such as: godaddy application list or godaddy actions list.","next_actions":[...]}
```

`--help` remains standard CLI help text.
`--output` has been removed; all executable command paths return JSON envelopes.
Use `--pretty` to format envelopes with 2-space indentation for human readability.
Long-running operations can stream typed NDJSON events with `--follow`, ending with a terminal `result` or `error` event.

## Root Discovery

```bash
godaddy
```

Returns environment/auth snapshots and the full command tree.

## Global Options

- `-e, --env <environment>`: validate target environment (`ote`, `prod`)
- `--debug`: enable debug logging (stderr only)
- `--pretty`: pretty-print JSON envelopes (2-space indentation)

## Commands

### Environment

- `godaddy env`
- `godaddy env list`
- `godaddy env get`
- `godaddy env set <environment>`
- `godaddy env info [environment]`

### Authentication

- `godaddy auth`
- `godaddy auth login`
- `godaddy auth logout`
- `godaddy auth status`

### Application

- `godaddy application` (alias: `godaddy app`)
- `godaddy application list` (alias: `godaddy app ls`)
- `godaddy application info <name>`
- `godaddy application validate <name>`
- `godaddy application update <name> [--label <label>] [--description <description>] [--status <status>]`
- `godaddy application enable <name> --store-id <storeId>`
- `godaddy application disable <name> --store-id <storeId>`
- `godaddy application archive <name>`
- `godaddy application init [--name <name>] [--description <description>] [--url <url>] [--proxy-url <proxyUrl>] [--scopes <scopes>] [--config <path>] [--environment <env>]`
  - `--url` and `--proxy-url` must be publicly-resolvable `http(s)` URLs. `localhost`, loopback (`127.0.0.1`, `::1`), link-local, and RFC1918 private IPs are rejected. For local development, expose a tunnel (e.g. [cloudflared](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/), [ngrok](https://ngrok.com/)) and register the tunnel hostname.
- `godaddy application release <name> --release-version <version> [--description <description>] [--config <path>] [--environment <env>]`
- `godaddy application deploy <name> [--config <path>] [--environment <env>] [--follow]`

#### Application Add

- `godaddy application add`
- `godaddy application add action --name <name> --url <url>`
- `godaddy application add subscription --name <name> --events <events> --url <url>`
- `godaddy application add extension`
- `godaddy application add extension embed --name <name> --handle <handle> --source <source> --target <targets>`
- `godaddy application add extension checkout --name <name> --handle <handle> --source <source> --target <targets>`
- `godaddy application add extension blocks --source <source>`

### Webhooks

- `godaddy webhook`
- `godaddy webhook events`

### Actions

- `godaddy actions`
- `godaddy actions list`
- `godaddy actions describe <action>`

## Development

```bash
pnpm install
pnpm run build
pnpm test
```
