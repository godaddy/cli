#!/usr/bin/env bash
# Fails if any hand-written .rs file exceeds the line-count limit — see
# docs/code-structure.md for the file-layout convention this enforces.
#
# Scope: rust/src, every rust/tools/*/src, and rust/domains-client/src — every
# workspace member's hand-written source. Deliberately excludes rust/target
# (build output; nothing under it is committed source).
set -euo pipefail

rust_root="$(cd "$(dirname "$0")/.." && pwd)"
limit=1000
violations=0

while IFS= read -r -d '' file; do
  lines=$(wc -l < "$file")
  if [ "$lines" -gt "$limit" ]; then
    echo "  $file: $lines lines (limit $limit)"
    violations=$((violations + 1))
  fi
done < <(find "$rust_root/src" "$rust_root"/tools/*/src "$rust_root/domains-client/src" \
  -name '*.rs' -print0 2>/dev/null)

if [ "$violations" -gt 0 ]; then
  echo "ERROR: $violations file(s) over the ${limit}-line limit (shown above)."
  echo "Split by subcommand/concern — see docs/code-structure.md."
  exit 1
fi

echo "==> All .rs files are within the ${limit}-line limit"
