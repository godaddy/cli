# fix(extensions): restore pre-bundle AST security scan [DEVEX-710]

## Summary

- Restore pre-bundle oxc AST scanning (SEC001–SEC010/SEC012) plus SEC011 package-script scanning under `extension/security/`, with `scan_extension` returning `Result<ScanReport, ScanError>` so discovery/read/parse/symlink failures fail closed (separate from rule findings).
- Wire `scan.prebundle` into deploy **before** esbuild; block on severity `Block`, then keep the existing post-bundle regex scan (SEC101–SEC115).
- SEC012 parity: DOM pair/triple blocklists, scope shadowing, and host `mount({ container })` still catching container escapes (`closest`, `ownerDocument`, etc.) without treating `container` as a free-variable shadow.
- Hardening follow-ups: UTF-8-safe snippet/offset helpers, deterministic finding sort (`file`/`line`/`col`/`rule_id`), fail-closed file discovery (read_dir / symlink), clearer deploy `--help` (AST vs package scripts).

## Test plan

- [x] `cargo check` / `cargo clippy -- -D warnings` / `cargo fmt --check` / `./rust/scripts/check-module-size.sh`
- [x] `cargo test` (614 tests; includes ~139 `extension::security` tests)
- [x] Fail-closed coverage: unreadable dirs, symlinks, oxc parse errors, invalid/unreadable `package.json`
- [x] Manual fixture scan (pre-bundle only):
  - `clean` / `shadow-ok` → pass (0 findings)
  - `eval-bad` → blocked (SEC001)
  - `dom-bad` → blocked (SEC012, 3 findings)
- [ ] End-to-end `gddy platform app deploy` on a real app (needs experimental stage + auth + esbuild): confirm pre-bundle blocks bad packages and clean packages proceed past scan to upload
