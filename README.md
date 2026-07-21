# GoDaddy CLI

Agent-first CLI for interacting with GoDaddy Developer Platform.

## Installation

Download the latest release binary from the [releases page](https://github.com/godaddy/cli/releases) and place it on your `PATH`.

```bash
gddy --help
```

### Beta (Rust port preview)

The CLI is being rewritten in Rust on the `rust-port` branch with expanded functionality. An experimental **`gddy`** binary is available that can be installed **alongside** the current `godaddy` CLI, so you can try it without disturbing your existing setup.

To install it, run the following.

**macOS / Linux (and Git Bash / MSYS2 / Cygwin on Windows):**

```bash
curl -fsSL https://github.com/godaddy/cli/releases/latest/download/install.sh | bash
gddy --version
```

**Windows (PowerShell):**

```powershell
irm https://github.com/godaddy/cli/releases/latest/download/install.ps1 | iex
gddy --version
```

Both installers download, checksum-verify, and install the binary for your
platform (`gddy.exe` on Windows). If you'd rather install by hand, download the
`gddy-x86_64-pc-windows-msvc.zip` asset from the [latest release](https://github.com/godaddy/cli/releases/latest) and put `gddy.exe` on your `PATH`.

## Output Contract

All executable commands emit JSON envelopes:

```json
{"ok":true,"command":"gddy env get","result":{"environment":"ote"},"next_actions":[...]}
```

```json
{"ok":false,"command":"gddy application info demo","error":{"message":"Application 'demo' not found","code":"NOT_FOUND"},"fix":"Use discovery commands such as: gddy application list or gddy actions list.","next_actions":[...]}
```

`--help` remains standard CLI help text.
`--output` has been removed; all executable command paths return JSON envelopes.
Long-running operations can stream typed NDJSON events with `--follow`, ending with a terminal `result` or `error` event.

## Root Discovery

```bash
gddy
```

Returns environment/auth snapshots and the full command tree.

## Global Options

- `-e, --env <environment>`: validate target environment (`ote`, `prod`)
- `--debug`: enable debug logging (stderr only)

## Commands

### Environment

- `gddy env`
- `gddy env list`
- `gddy env get`
- `gddy env set <environment>`
- `gddy env info [environment]`

### Authentication

- `gddy auth`
- `gddy auth login`
- `gddy auth logout`
- `gddy auth status`

### Application

- `gddy application` (alias: `gddy app`)
- `gddy application list` (alias: `gddy app ls`)
- `gddy application info <name>`
- `gddy application validate <name>`
- `gddy application update <name> [--label <label>] [--description <description>] [--status <status>]`
- `gddy application enable <name> --store-id <storeId>`
- `gddy application disable <name> --store-id <storeId>`
- `gddy application archive <name>`
- `gddy application init [--name <name>] [--description <description>] [--url <url>] [--proxy-url <proxyUrl>] [--scopes <scopes>] [--config <path>] [--environment <env>]`
  - `--url` and `--proxy-url` must be publicly-resolvable `http(s)` URLs. `localhost`, loopback (`127.0.0.1`, `::1`), link-local, and RFC1918 private IPs are rejected. For local development, expose a tunnel (e.g. [cloudflared](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/), [ngrok](https://ngrok.com/)) and register the tunnel hostname.
- `gddy application release <name> --release-version <version> [--description <description>] [--config <path>] [--environment <env>]`
- `gddy application deploy <name> [--config <path>] [--environment <env>] [--follow]`

#### Application Add

- `gddy application add`
- `gddy application add action --name <name> --url <url>`
- `gddy application add subscription --name <name> --events <events> --url <url>`
- `gddy application add extension`
- `gddy application add extension embed --name <name> --handle <handle> --source <source> --target <targets>`
- `gddy application add extension checkout --name <name> --handle <handle> --source <source> --target <targets>`
- `gddy application add extension blocks --source <source>`

### Payment Methods

- `gddy payment-methods`
- `gddy payment-methods add` — opens your default browser to the GoDaddy payment methods management page. Only credit card or Good-as-Gold can be used for domain purchases.

### Webhooks

- `gddy webhook`
- `gddy webhook events`

### Actions

- `gddy actions`
- `gddy actions list`
- `gddy actions describe <action>`

## Development

```bash
cd rust
cargo build
cargo test
cargo clippy -- -D warnings
```
