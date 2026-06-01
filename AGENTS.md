# Agent Notes

## Language and Runtime
- All source code is Rust (edition 2024).
- Binary: `godaddy` — built with `cargo build` from `rust/`.

## Command Patterns (Required)
- Commands are `RuntimeCommandSpec` (or `RuntimeGroupSpec` for groups).
- Register commands via `Module::new(...)` and wire them in `main.rs`.
- Retrieve arguments via `ctx.args.get("key")`.
- Return `Ok(CommandResult::new(json!({...})))` for success.
- Return `Err(cli_engine::CliCoreError::message("..."))` for user-facing errors.
- Streaming commands use `RuntimeCommandSpec::new_streaming` and emit events via `StreamSender`.

## Verification Checklist
- `cargo check` — must pass
- `cargo clippy -- -D warnings` — must pass with zero warnings
- `cargo test` — must pass
- `cargo fmt --check` — must be clean

## Regenerating the API Catalog
```bash
cd rust
cargo run -p generate-api-catalog
```
This clones upstream OpenAPI specs and writes `rust/schemas/api/*.json`.
Set `GITHUB_TOKEN` for full repo discovery; falls back to a hardcoded bootstrap list otherwise.
