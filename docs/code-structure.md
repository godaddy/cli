# Code file structure

`gddy`'s Rust source should mirror its CLI command tree. This doc explains the target shape and the CI check that enforces it, with a worked example of what to do — and what not to do.

## The rule

- One file per leaf subcommand, named after the subcommand (`domain/purchase.rs` for `gddy domain purchase`).
- One `mod.rs` per command group, containing *only* wiring: `mod` declarations, a `pub fn module()` (top level) or `pub(super) fn group()` (nested), and nothing that could stand alone as a unit of behavior.
- Shared cross-cutting helpers (an authenticated client, error-body rendering, money/format helpers, validation) live in a sibling `common.rs`, exposed at `pub(super)`/`pub(crate)` — not duplicated per subcommand file, and not left in `mod.rs`. If the `common.rs` file grows too large, like over 1000 lines of code, split it in to appropriately named modules per theme.
- A nested group (a subcommand that itself has subcommands, e.g. `domain nameservers set`) gets its own file exporting `pub(super) fn group() -> RuntimeGroupSpec`. If that group's own commands grow past the point a flat file can hold comfortably, promote it to a subdirectory with its own `mod.rs` plus one file per leaf, the same way a top-level module works.
- CI enforces a 1000-line ceiling on every hand-written `.rs` file — see `rust/scripts/check-module-size.sh`. There's no allowlist; if a file is over the line, split it before merging.

## Example

```
src/domain/
  mod.rs            # Module::new("Domains", ...) wiring only
  common.rs         # make_client, format_money, validate_domain_name, api_error
  list.rs           # `domain list`
  get.rs            # `domain get`
  available.rs      # `domain available`
  suggest.rs        # `domain suggest`
  agreements.rs     # `domain agreements`
  quote.rs          # `domain quote`
  purchase.rs       # `domain purchase`
  nameservers.rs    # `domain nameservers set` (nested group, flat file)
  contacts.rs       # `domain contacts ...` (nested group, flat file)
  operation.rs      # `domain operation status` (nested group, flat file)
```

`mod.rs` never contains a `CommandSpec`, a `clap::Args` struct, or an API call — it only assembles what the sibling files export. A reviewer can find "the `purchase` command" by opening exactly one file, and never has to scroll past unrelated commands to get there.
