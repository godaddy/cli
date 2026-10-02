# GoDaddy CLI — Agent Notes

This is a command-line application. Source code is written in Rust and lives under `rust/`. Run `cargo` commands from that directory.

## Commands

- **Build**: `cargo build`
- **Run**: `cargo run -- <command>`
- **Test**: `cargo test --workspace` (the workspace has a root package, so plain
  `cargo test` silently skips the `domains-client` and `generate-api-catalog`
  members — always pass `--workspace`)
- **Lint**: `cargo clippy --workspace -- -D warnings`
- **Format**: `cargo fmt`
- **Check**: `cargo check --workspace`
- **Refresh API specs**: `cargo run -p generate-api-catalog`
  — set `SKIP_DOMAINS_REFRESH` to skip the domains-client spec pull for
  local iteration without network access.

## Verification Checklist (required before finishing work)

- `cargo check --workspace` — must pass
- `cargo clippy --workspace -- -D warnings` — must pass with zero warnings
- `cargo test --workspace` — must pass
- `cargo fmt --check` — must be clean
- `./rust/scripts/check-module-size.sh` — must pass

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

## Shopping post-purchase actions

- `rust/src/shopping/product_actions.rs` maps Shopping line-item categories to post-purchase CLI guidance.
- When adding a product purchasable through `shopping` that has a follow-up CLI flow, add its category-to-guide action there and cover it with a unit test. Keep the action list limited to product-specific next steps.

## Purchase confirmation (Required)

Commands that charge money or request user consent should follow the examples of the `domain purchase` and `shopping checkout complete` commands on insisting that agents require explicit consent from users.

## Command Patterns (Required)

- Commands are `RuntimeCommandSpec` (or `RuntimeGroupSpec` for groups).
- Register commands via `Module::new(...)` and wire them in `main.rs`.
- Use `clap::Arg` for arguments; retrieve via `ctx.args.get("key")`.
- Return `Ok(CommandResult::new(json!({...})))` for success.
- Prefer `crate::error::GddyError::{not_found,validation,auth,config,security,network,…}` (and `GddyError::from` for module client errors) so agents get stable `error.code` + top-level `fix`. Use `Err(cli_engine::CliCoreError::message("..."))` only for one-off cases that do not yet have a shared mapping.
- Streaming commands use `RuntimeCommandSpec::new_streaming` and emit events via `StreamSender`.
- Commands with external effects are marked mutating so the engine's dry-run safeguard applies. Dry-run paths validate and read every prerequisite the real call needs (so they fail where it would) and return `CommandResult::with_dry_run()`.
- Next actions (suggested follow-up commands) use a command template plus structured params, not a `format!`-built command line like `--query '{query}'` (a quote in the value breaks it; metacharacters can inject commands). Param names must match the target command's args and the template's `<placeholder>`s. Emit next actions only when executable and appropriate to the returned state, and never include consent-bypass flags (e.g. `--agree`) in them.
- Encode dynamic path segments with `api::http::encode_path_segment` when assembling URLs by hand. Generated (Progenitor) clients already percent-encode path parameters; pass them raw to avoid double encoding.
- Do not call `--debug transport` logging helpers for payloads that may hold customer, payment or order data.
- Constrain flags to the API's documented values at argument parsing, so bad input fails locally with clear help.
- Correctable input or config failures use a stable validation error with an actionable `fix`; never turn malformed config into an empty payload.
- Surface in-band API errors as errors and preserve empty acknowledgements as-is; do not substitute default "success" objects or cache errors as empty data.
- Polling/retry wrappers map only the exhausted expected status (e.g. 404) to `not_found`; keep 429/5xx/network errors as-is.
- Resolve the API base URL from the selected environment; do not add per-service URL overrides or `--env` flags on follow-up commands.

## Reuse Before You Build (Required)

Search the codebase and `cli-engine` before writing a helper; reviewers reject duplication.

- Typed clients: generate with Progenitor from the OpenAPI spec (as existing generated clients in the workspace do). Do not hand-write `reqwest` clients that traverse `serde_json::Value`.
- Rendering: prefer `cli-engine` rendering (`HumanViewDef`/`TableColumn`, structured next actions and its standard footer) over hand-formatted tables or local display logic. Only write custom rendering when `cli-engine` cannot express it.
- Shared formatting (money, etc.): reuse existing helpers rather than adding per-module copies.

## User-Facing Text (Required)

- Write help, guides and output for customers: no internal system or API names, scopes, environments or implementation jargon.
- Where an AI assistant must act differently from a human (e.g. consent before a charge), address it directly in a clearly marked `AI assistants:` note; never mix that into customer-facing prose.
- Command descriptions are short imperatives from the user's point of view; give flags concrete examples and discoverable values; show where prerequisite values (IDs) come from.
- Don't expose internals (retry mechanics, generated keys, etc.) in normal help or output; when something fails, put the suggested next step in the error `fix`.
- Avoid raw JSON inputs in the main flow. Guides should use soft line breaks (hard breaks only in shell examples).
- PR descriptions must match implemented behavior.

Full checklist: [Command authoring](./docs/command-authoring.md).

## Code File Structure (Required)

- Rust source files should mirror the CLI command tree structure. See 
[Code file structure](./docs/code-structure.md) for specifics.
- CI fails any `.rs` file over 1000 lines. Split file exceeding this limit.

## Key Concepts

### Authentication

- Handled entirely by `cli-engine` (PKCE OAuth flow, secure credential storage).
- The `ctx.credential` field on `CommandContext` provides the current token.

### Configuration

- Application settings in TOML format (`godaddy.toml`, `godaddy.<env>.toml`).
- Read/write via `crate::config::{read_config, write_config, config_path}`.

### Extension security scanner

- Pre-bundle AST scan in `extension/security/` (oxc): SEC001–SEC010, SEC012, plus
  SEC011 package scripts (`scan_extension`).
- Post-bundle regex scanner in `extension/security/mod.rs` (rule data in
  `extension/security/rules.rs`).
- Rules SEC101–SEC110 ported from the TS scanner; SEC111–SEC115 added in the
  Rust port with no TS baseline (SEC111/SEC112/SEC115 block, SEC113/SEC114
  warn). Uses `fancy-regex` for lookahead support.
- `scan_extension(dir) -> Result<ScanReport, ScanError>`, `scan_bundle(content, path) -> Vec<Finding>`,
  `is_blocked(findings) -> bool`.
- Deploy runs pre-bundle scan before esbuild, then post-bundle scan on the artifact.

### esbuild dependency

- The `bundle_extension` function spawns `esbuild` as a subprocess.
- It searches `node_modules/.bin/esbuild` walking up from CWD, then falls back to PATH.
- Users need esbuild available (via `npm`/`pnpm` install or globally).
