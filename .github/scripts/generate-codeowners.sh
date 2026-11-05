#!/usr/bin/env bash
# Regenerates .github/CODEOWNERS from reviewers.json. Run this after
# editing that file, then commit the result — .github/workflows/codeowners-drift.yml
# fails the build if they fall out of sync.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
data_file="$script_dir/reviewers.json"
out_file="$script_dir/../CODEOWNERS"

data_json="$(cat "$data_file")"
infra_owners="$(jq -r '.infra.users | map("@" + .) | join(" ")' <<<"$data_json")"
fallback_pattern="$(jq -r '.fallbackPattern' <<<"$data_json")"

{
  cat <<'HEADER'
# Reviewer routing for godaddy/cli.
#
# GENERATED FILE — do not hand-edit. Source of truth is
# .github/scripts/reviewers.json; regenerate with
# .github/scripts/generate-codeowners.sh (checked by
# .github/workflows/codeowners-drift.yml).
#
# This file drives GitHub's native "auto-request reviewers" behavior and, if
# "Require review from Code Owners" is enabled in branch protection, a
# check satisfied by ONE approval from any owner listed on a matching line
# (GitHub does not support AND-semantics across owners on the same line).
# Infra + product owners are both listed/requested per path below, but
# getting both to actually sign off before merging is an honor-system
# expectation, not something this file enforces.
HEADER
  echo
  echo "# Infra fallback — cross-cutting CLI concerns with no specific product owner."
  echo "$fallback_pattern $infra_owners"
  echo

  echo "# Product paths with an assigned team, if one has been assigned yet — infra always requested, plus the product team's owners once its users list is non-empty."
  echo
  jq -c '.productPaths[]' <<<"$data_json" | while read -r group; do
    label="$(jq -r '.label' <<<"$group")"
    comment="$(jq -r '.comment // ""' <<<"$group")"
    group_owners="$(jq -r '.users | map("@" + .) | join(" ")' <<<"$group")"
    if [[ -n "$comment" ]]; then
      echo "# $label — $comment"
    else
      echo "# $label"
    fi
    jq -r '.patterns[]' <<<"$group" | while read -r pattern; do
      if [[ -n "$group_owners" ]]; then
        echo "$pattern $infra_owners $group_owners"
      else
        echo "$pattern $infra_owners"
      fi
    done
    echo
  done
} > "$out_file"

echo "Wrote $out_file"
