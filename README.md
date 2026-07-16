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

Regular executable commands in JSON mode emit GoDaddy JSON envelopes:

```json
{"command":"gddy env get","next_actions":[],"ok":true,"result":{"apiUrl":"https://api.godaddy.com","env":"prod"}}
```

```json
{"command":"gddy env get","error":{"code":"ERROR","message":"unknown environment \"not-an-environment\"; known: ote, prod"},"next_actions":[],"ok":false}
```

Regular command envelopes, including errors, are written to stdout. Failed
commands retain a non-zero exit code. `--help` and `--version` remain standard
CLI text, and non-JSON diagnostics retain their original stream.

JSON is the default for non-interactive output. Use `--output
<json|human|toon>` to select a format; `--json`, `--human`, and `--toon` are
shorthands. Human and TOON output pass through without GoDaddy JSON-envelope
adaptation. JSON envelopes use two-space indentation by default. `--pretty` is
not registered. Streaming commands are outside this regular-command adapter;
terminal `result` and `error` events are tracked separately.

## Root Discovery

```bash
gddy
```

Returns environment/auth snapshots and the full command tree.

## Global Options

- `--env <environment>`: validate target environment (`ote`, `prod`)
- `--debug`: enable debug logging (stderr only)
- `--output <json|human|toon>`: select the output format
- `--json`, `--human`, `--toon`: output-format shorthands

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
- `gddy application deploy <name> [--config <path>] [--environment <env>]`

#### Application Add

- `gddy application add`
- `gddy application add action --name <name> --url <url>`
- `gddy application add subscription --name <name> --events <events> --url <url>`
- `gddy application add extension`
- `gddy application add extension embed --name <name> --handle <handle> --source <source> --target <targets>`
- `gddy application add extension checkout --name <name> --handle <handle> --source <source> --target <targets>`
- `gddy application add extension blocks --source <source>`

### Payments

- `gddy payments`
- `gddy payments add` — opens your default browser to the GoDaddy payment methods management page. Only credit card or Good-as-Gold can be used for domain purchases.

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
