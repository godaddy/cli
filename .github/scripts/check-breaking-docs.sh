#!/usr/bin/env bash
# Fails a pull request that marks a breaking change but doesn't document it in the
# upgrade guide (`gddy guide upgrades`, rust/src/update/guides/upgrades.md).
#
# A pull request is breaking when its title has a `!` before the colon
# (`fix(dns)!: ...`), or when any of its commits has a `BREAKING CHANGE:` footer or a
# subject with that `!`. Those are the markers release-please turns into a breaking
# release. A breaking pull request has to change the guide, and the guide has to hold
# the section for the upgrade the change belongs to: `## 0.2 to 0.3` while the version
# in rust/Cargo.toml is 0.2.x, `## 1 to 2` while it is 1.x.y.
#
# Inputs, all from the environment:
#   PR_TITLE            the pull request title
#   COMMITS_FILE        the full message of every commit in the pull request
#   CHANGED_FILES_FILE  the path of every file the pull request changes, one per line
#   CARGO_TOML          the crate manifest holding the current version (default rust/Cargo.toml)
#   GUIDE               the upgrade guide (default rust/src/update/guides/upgrades.md)
#
# Run it from the repository root. `check-breaking-docs-test.sh` covers it.
set -euo pipefail

title=${PR_TITLE:?PR_TITLE is required}
commits_file=${COMMITS_FILE:?COMMITS_FILE is required}
changed_files_file=${CHANGED_FILES_FILE:?CHANGED_FILES_FILE is required}
cargo_toml=${CARGO_TOML:-rust/Cargo.toml}
guide=${GUIDE:-rust/src/update/guides/upgrades.md}

# `type!:` or `type(scope)!:`, the way a conventional commit header marks a break.
bang_header='^[A-Za-z]+(\([^)]*\))?!:'

reasons=()
if [[ $title =~ $bang_header ]]; then
  reasons+=("the title has a \`!\` before the colon")
fi
if grep -Eq "$bang_header" "$commits_file"; then
  reasons+=("a commit subject has a \`!\` before the colon")
fi
if grep -Eq '^BREAKING[ -]CHANGE:' "$commits_file"; then
  reasons+=("a commit has a \`BREAKING CHANGE:\` footer")
fi

if [[ ${#reasons[@]} -eq 0 ]]; then
  echo "Not a breaking change, so no upgrade notes are needed."
  exit 0
fi

version=$(awk -F'"' '/^version = /{print $2; exit}' "$cargo_toml")
IFS=. read -r major minor _ <<<"$version"
if [[ ! $major =~ ^[0-9]+$ || ! $minor =~ ^[0-9]+$ ]]; then
  echo "::error::Can't read the crate version from $cargo_toml (found \"$version\")."
  exit 2
fi

# The upgrade this change ships in: one minor up while the major is 0, else one major up.
if [[ $major -eq 0 ]]; then
  from="0.$minor"
  to="0.$((minor + 1))"
else
  from="$major"
  to="$((major + 1))"
fi
heading="## $from to $to"

problems=()
if ! grep -Fxq -- "$guide" "$changed_files_file"; then
  problems+=("This pull request doesn't change $guide.")
fi
if [[ ! -f $guide ]]; then
  problems+=("$guide doesn't exist.")
elif ! sed 's/[[:space:]]*$//' "$guide" | grep -Fxq -- "$heading"; then
  problems+=("$guide has no section titled \`$heading\`.")
fi

if [[ ${#problems[@]} -eq 0 ]]; then
  echo "Breaking change documented under \"$heading\"."
  exit 0
fi

{
  joined=$(printf '%s; ' "${reasons[@]}")
  echo "This pull request is a breaking change (${joined%; }), so it has to say"
  echo "what users change."
  echo
  for problem in "${problems[@]}"; do
    echo "  - $problem"
  done
  echo
  echo "Add or extend the \"$heading\" section in $guide. For each affected"
  echo "command, flag, variable or field, say what it was, what it is now, and"
  echo "what to do. See .agents/skills/breaking-changes/SKILL.md. If this"
  echo "change isn't breaking, remove the\`!\` from the title and the"
  echo "\`BREAKING CHANGE:\` footer from the commits."
} >&2
echo "::error title=Breaking change has no upgrade notes::Add a \"$heading\" section to $guide."
exit 1
