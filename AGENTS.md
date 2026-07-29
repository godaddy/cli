# GoDaddy CLI — Agent Notes

This is a command-line application. Source code is written in Rust and lives under `rust/`. Run `cargo` commands from that directory.

## Commands

- **Build**: `cargo build`
- **Run**: `cargo run -- <command>`
- **Test**: `cargo test`
- **Lint**: `cargo clippy -- -D warnings`
- **Format**: `cargo fmt`
- **Check**: `cargo check`
- **Regenerate API catalog**: `cargo run -p generate-api-catalog`
  — clones upstream OpenAPI specs and writes `rust/schemas/api/*.json`. Set
  `GITHUB_TOKEN` for full repo discovery; falls back to a hardcoded bootstrap
  list otherwise.
- **Regenerate Node.js Hosting spec**: `./rust/scripts/regenerate-hosting-spec.sh`

## Verification Checklist (required before finishing work)

- `cargo check` — must pass
- `cargo clippy -- -D warnings` — must pass with zero warnings
- `cargo test` — must pass
- `cargo fmt --check` — must be clean

## Architecture

GoDaddy CLI is a Rust binary (edition 2024) built using:

- **cli-engine**: Command registration, auth (PKCE OAuth), credential storage, streaming
- **clap**: Argument parsing (via cli-engine's `CommandSpec`/`GroupSpec`)
- **reqwest**: HTTP client with rustls-tls
- **serde_json**: JSON serialization and API payloads
- **tokio**: Async runtime

## Code Style

- **Rust edition 2024**. Follow existing patterns in the codebase.
- Lints are enforced via `Cargo.toml` (`[lints.rust]` and `[lints.clippy]`):
  - `unsafe_code = "deny"`, `unwrap_used = "deny"`, `exit = "deny"`
  - Use `.expect("reason")` instead of `.unwrap()`
  - No `println!`/`eprintln!` — use `tracing` or `cli-engine` event streams
- Keep functions focused; avoid premature abstractions.

## Command Patterns (Required)

- Commands are `RuntimeCommandSpec` (or `RuntimeGroupSpec` for groups).
- Register commands via `Module::new(...)` and wire them in `main.rs`.
- Use `clap::Arg` for arguments; retrieve via `ctx.args.get("key")`.
- Return `Ok(CommandResult::new(json!({...})))` for success.
- Prefer `crate::error::GddyError::{not_found,validation,auth,config,security,network,…}` (and `GddyError::from` for module client errors) so agents get stable `error.code` + top-level `fix`. Use `Err(cli_engine::CliCoreError::message("..."))` only for one-off cases that do not yet have a shared mapping.
- Streaming commands use `RuntimeCommandSpec::new_streaming` and emit events via `StreamSender`.

## Key Concepts

### Authentication

- Handled entirely by `cli-engine` (PKCE OAuth flow, secure credential storage).
- The `ctx.credential` field on `CommandContext` provides the current token.

### Configuration

- Application settings in TOML format (`godaddy.toml`, `godaddy.<env>.toml`).
- Read/write via `crate::config::{read_config, write_config, config_path}`.

### Extension security scanner

- Post-bundle regex scanner in `extension/mod.rs`.
- Rules SEC101–SEC115; uses `fancy-regex` for lookahead support.
- `scan_bundle(content, path) -> Vec<Finding>`, `is_blocked(findings) -> bool`.

### esbuild dependency

- The `bundle_extension` function spawns `esbuild` as a subprocess.
- It searches `node_modules/.bin/esbuild` walking up from CWD, then falls back to PATH.
- Users need esbuild available (via `npm`/`pnpm` install or globally).
