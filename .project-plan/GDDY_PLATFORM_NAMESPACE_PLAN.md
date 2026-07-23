# GDDY Platform Namespace Plan

## Decision

Replace the Rust CLI's root-level GoDaddy Platform Applications (GPA) command
families with one `gddy platform` namespace. The canonical commands become:

```text
gddy platform app init
gddy platform actions list
gddy platform webhook events
```

`app` is the canonical resource name. `application` may be retained only as an
alias *below* `platform` (`gddy platform application ...`) if the command engine
supports it cleanly; the ambiguous root-level `gddy application ...` path is not
retained.

This implements the agreed naming direction without changing the executable
name (`gddy`), app-registry API contracts, OAuth scopes, manifests, or command
behavior.

## Current state

On `rust-port` (`6abd2ee`), the three GPA-owned command groups are registered
as separate top-level modules:

| Current command family | Owner | Existing feature flag |
| --- | --- | --- |
| `gddy application` / `gddy app` | `rust/src/application` | `application` (Experimental) |
| `gddy actions` | `rust/src/actions_catalog` | `actions` (Experimental) |
| `gddy webhook` | `rust/src/webhook` | `webhook` (Experimental) |

`rust/src/main.rs` mounts each module independently. Their code, README
examples, root next action, help copy, and `NextAction` suggestions still emit
the old root-level paths. `cli-engine` supports nested `RuntimeGroupSpec`
groups and recursively registers their command paths, so this is a command-tree
restructure rather than a transport or business-logic rewrite.

## Target command tree

```text
gddy
└── platform                         [Experimental]
    ├── app (alias: application)
    │   ├── list | info | init | validate | update | enable | disable
    │   ├── archive | release | deploy
    │   └── add action | subscription | extension ...
    ├── actions
    │   └── list | describe
    └── webhook
        └── events
```

All non-GPA root groups, such as `domain`, `dns`, `api`, `hosting`, and
`payments`, remain unchanged. The implementation does **not** rename
application-domain types, GraphQL operations, `godaddy.toml`, config keys, API
URLs, or OAuth scopes.

## Implementation plan

1. Add a platform composition module.

   - Create `rust/src/platform/mod.rs` and register it from `rust/src/main.rs`.
   - Build one `RuntimeGroupSpec` named `platform` with clear GoDaddy Developer
     Platform help text.
   - Attach the existing application, action-catalog, and webhook groups as
     nested children, then remove their three independent root module
     registrations from `main.rs`.
   - Apply `Stage::Experimental` to the `platform` group with a `platform`
     feature key. This keeps the preview surface hidden under the default
     feature policy and prevents an empty `platform` group from appearing when
     its children are unavailable.

2. Make the existing GPA groups composable beneath `platform`.

   - Change `rust/src/application/mod.rs` and
     `rust/src/application/commands/mod.rs` so they expose the application
     runtime group for composition instead of only returning a top-level
     `Module`.
   - Rename the group to `app`; make `application` a nested alias only if
     verified against the installed `cli-engine` version.
   - Refactor `rust/src/actions_catalog/mod.rs` and `rust/src/webhook/mod.rs`
     to expose their existing `RuntimeGroupSpec` values. Preserve their
     handlers, authentication, `Tier`, schemas, and API clients unchanged.
   - Remove the now-redundant module-level `application`, `actions`, and
     `webhook` feature declarations. The one parent `platform` declaration is
     the visibility contract for the regrouped GPA surface.

3. Rewrite every user-facing command reference to the canonical tree.

   - Update root long help and root next actions in `rust/src/main.rs`.
   - Update command long help, validation/error remediation, and all
     `next_action(...)` values in `rust/src/application/commands/mod.rs`,
     `rust/src/actions_catalog/mod.rs`, and `rust/src/webhook/mod.rs`.
   - Update the application-scope comments and examples in `rust/src/scopes.rs`
     where they show CLI paths.
   - Restructure the README command reference into a **Developer Platform**
     section with the three namespaced groups, canonical `app` examples, and a
     concise migration note: replace `gddy application ...`, `gddy actions ...`,
     and `gddy webhook ...` with their `gddy platform ...` equivalents.
   - Use a targeted repository search for old executable command strings as a
     completion gate. Do not change prose that uses “application” as an API or
     domain noun rather than a CLI path.

4. Add regression coverage at the command-tree boundary.

   - Extend or add a focused Rust test that constructs the CLI with
     `Stage::Experimental` enabled and asserts `gddy tree --output json`
     publishes `platform`, then `app`, `actions`, and `webhook` at the expected
     nested paths.
   - Exercise `gddy platform app --help`, `gddy platform actions --help`, and
     `gddy platform webhook --help`; assert each help page shows its existing
     subcommands.
   - Preserve the existing fail-closed authentication test for application
     listing, updated to invoke `gddy platform app list`.
   - Add an explicit negative assertion that `gddy application` is no longer a
     registered root command. If the nested `application` alias is kept, verify
     only `gddy platform application` resolves.

## Migration and release notes

This is intentionally a command-path breaking change. The Rust `gddy` binary
is described in the repository README as a Rust-port preview that installs
alongside the legacy `godaddy` CLI, making this the appropriate time to remove
the ambiguous root namespace instead of carrying two competing discovery paths.

The release note should include a three-line migration table and call out that
scripts, agent prompts, and docs must switch to the new paths. No data migration
or server-side rollout is required.

| Before | After |
| --- | --- |
| `gddy application init` | `gddy platform app init` |
| `gddy actions list` | `gddy platform actions list` |
| `gddy webhook events` | `gddy platform webhook events` |

## Risks and guardrails

- **Broken automation or prompts:** command paths are machine-consumed through
  JSON `next_actions`; update those strings in the same change and test the
  tree, help, and representative next actions together.
- **Feature-flag regression:** moving three Experimental modules under one
  parent changes the override key to `platform`. Document this in the release
  note and verify default discovery remains free of an empty parent group.
- **Accidental scope expansion:** do not move non-GPA roots or alter API
  clients, credentials, GraphQL queries, configuration layout, or output
  schemas.
- **Alias ambiguity:** only keep `gddy platform application` if its parser and
  discovery behavior are verified. It must not reintroduce `gddy application`
  at the root.

## Verification

From `rust/`, run:

```bash
cargo fmt --check
cargo check
cargo clippy -- -D warnings
cargo test
```

Then manually inspect the Experimental tree and help under the repository's
supported feature-policy setup, plus a representative no-auth action command:

```bash
gddy tree --output json
gddy platform app --help
gddy platform actions list --output json
gddy platform webhook --help
```

Confirm the legacy root `gddy application`, `gddy actions`, and `gddy webhook`
paths are rejected and that the canonical commands place `platform` in every
envelope and next-action command string.
