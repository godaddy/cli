#!/usr/bin/env bash
# Run check/clippy/fmt/module-size (no tests), then build and run the `gddy`
# binary. Any arguments passed to this script are forwarded to `gddy` at the
# end (e.g. `./scripts/build-and-verify.sh platform app --help`); with no
# arguments it just runs `gddy --help` as a smoke check.
set -euo pipefail

rust_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$rust_root"

step() {
  echo
  echo "==> $1"
}

step "cargo check"
cargo check

step "cargo clippy -- -D warnings"
cargo clippy -- -D warnings

step "cargo fmt --check"
cargo fmt --check

step "check-module-size.sh"
./scripts/check-module-size.sh

step "cargo build"
cargo build

step "gddy ${*:-\"--help\"}"
if [ "$#" -gt 0 ]; then
  ./target/debug/gddy "$@"
else
  ./target/debug/gddy --help
fi
