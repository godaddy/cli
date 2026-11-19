#!/usr/bin/env bash
# Fails a pull request that marks a breaking change but doesn't document it in the
# upgrade guide (`gddy guide upgrades`, rust/src/update/guides/upgrades.md).
#
# A pull request is breaking when its title has a `!` before the colon
# (`fix(dns)!: ...`), or when any of its commits has a subject with that `!` or a
# `BREAKING CHANGE:` footer. Those are the markers release-please turns into a breaking
# release. Only a commit's first line is a subject: a body that quotes `fix!: ...` as an
# example doesn't count.
#
# The guide covers the `gddy` CLI, which release-please versions from the files under
# `rust/`. The GitHub Action in `action/` is released on its own, so a breaking change
# that doesn't touch `rust/` doesn't change the CLI and needs no upgrade notes here.
# the section for the upgrade the change belongs to: `## 0.2 to 0.3` while the version
# in rust/Cargo.toml is 0.2.x, `## 1 to 2` while it is 1.x.y.
#
# Inputs, all from the environment:
#   PR_TITLE            the pull request title
#   SUBJECTS_FILE       the subject (first line) of every commit in the pull request
#   COMMITS_FILE        the full message of every commit in the pull request
#   CHANGED_FILES_FILE  the path of every file the pull request changes, one per line
#   CLI_PATH            the directory holding the CLI package (default rust/)
#   CARGO_TOML          the crate manifest holding the current version (default rust/Cargo.toml)
#   GUIDE               the upgrade guide (default rust/src/update/guides/upgrades.md)
#
# Run it from the repository root. `check-breaking-docs-test.sh` covers it.
set -euo pipefail

title=${PR_TITLE:?PR_TITLE is required}
subjects_file=${SUBJECTS_FILE:?SUBJECTS_FILE is required}
commits_file=${COMMITS_FILE:?COMMITS_FILE is required}
changed_files_file=${CHANGED_FILES_FILE:?CHANGED_FILES_FILE is required}
cli_path=${CLI_PATH:-rust/}
cargo_toml=${CARGO_TOML:-rust/Cargo.toml}
guide=${GUIDE:-rust/src/update/guides/upgrades.md}

# `type!:` or `type(scope)!:`, the way a conventional commit header marks a break.
bang_header='^[A-Za-z]+(\([^)]*\))?!:'

reasons=()
if [[ $title =~ $bang_header ]]; then
  reasons+=("the title has a \`!\` before the colon")
fi
if grep -Eq "$bang_header" "$subjects_file"; then
  reasons+=("a commit subject has a \`!\` before the colon")
fi
if grep -Eq '^BREAKING[ -]CHANGE:' "$commits_file"; then
  reasons+=("a commit has a \`BREAKING CHANGE:\` footer")
fi

if [[ ${#reasons[@]} -eq 0 ]]; then
  echo "Not a breaking change, so no upgrade notes are needed."
  exit 0
fi

# A breaking change that leaves the CLI's files alone ships in another release.
touches_cli=false
while IFS= read -r changed; do
  if [[ $changed == "$cli_path"* ]]; then
    touches_cli=true
    break
  fi
done <"$changed_files_file"
if [[ $touches_cli == false ]]; then
  echo "Breaking change, but it doesn't change anything under $cli_path, so the gddy CLI" \
    "isn't affected and needs no upgrade notes."
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
