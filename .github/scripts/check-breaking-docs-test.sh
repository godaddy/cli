#!/usr/bin/env bash
# Tests check-breaking-docs.sh against fixture pull requests. Run it from anywhere:
#   bash .github/scripts/check-breaking-docs-test.sh
set -euo pipefail

script="$(cd "$(dirname "$0")" && pwd)/check-breaking-docs.sh"
guide_path=rust/src/update/guides/upgrades.md
failures=0

# check NAME WANT_EXIT TITLE COMMITS FILES VERSION GUIDE_TEXT [WANT_OUTPUT [SUBJECTS]]
# Runs the script in a scratch repository laid out like this one. COMMITS is the full
# text of the commit messages. SUBJECTS is the subject of each commit, one per line;
# it defaults to the first line of COMMITS, which is right for a single commit.
check() {
  local name=$1 want=$2 title=$3 commits=$4 files=$5 version=$6 guide_text=$7 want_output=${8:-}
  local subjects=${9-}
  local dir got output
  if [[ $# -lt 9 ]]; then
    subjects=$(printf '%s' "$commits" | head -n 1)
  fi
  dir=$(mktemp -d)
  mkdir -p "$dir/rust/src/update/guides"
  printf '%s\n' "$subjects" >"$dir/subjects"
  printf '%s' "$commits" >"$dir/commits"
  printf '%s' "$files" >"$dir/files"
  printf '[package]\nname = "x"\nversion = "%s"\n' "$version" >"$dir/rust/Cargo.toml"
  printf '%s' "$guide_text" >"$dir/$guide_path"
  set +e
  output=$(cd "$dir" && PR_TITLE=$title SUBJECTS_FILE=$dir/subjects COMMITS_FILE=$dir/commits CHANGED_FILES_FILE=$dir/files bash "$script" 2>&1)
  got=$?
  set -e
  rm -rf "$dir"
  if [[ $got -ne $want ]]; then
    echo "FAIL  $name: exit $got, wanted $want"
    echo "$output" | sed 's/^/        /'
    failures=$((failures + 1))
  elif [[ -n $want_output && $output != *"$want_output"* ]]; then
    echo "FAIL  $name: output has no \"$want_output\""
    echo "$output" | sed 's/^/        /'
    failures=$((failures + 1))
  else
    echo "ok    $name"
  fi
}

guide_02_to_03=$'# Upgrade\n\n## 0.1 to 0.2\n\ntext\n\n## 0.2 to 0.3\n\nmore\n'
guide_old_only=$'# Upgrade\n\n## 0.1 to 0.2\n\ntext\n'
guide_1_to_2=$'# Upgrade\n\n## 0.9 to 1.0\n\n## 1 to 2\n\ntext\n'
touched="$guide_path"$'\nrust/src/dns/add.rs\n'
untouched=$'rust/src/dns/add.rs\n'

check "a plain fix needs nothing" 0 \
  "fix(dns): keep an HTTPS root target" "fix(dns): keep an HTTPS root target"$'\n' "$untouched" 0.2.39 "$guide_old_only" \
  "Not a breaking change"
check "a feat with no marker needs nothing" 0 \
  "feat(update): add an upgrades guide" "feat(update): add an upgrades guide"$'\n\nBody text.\n' "$untouched" 0.2.39 "$guide_old_only"
check "the word breaking in prose is not a marker" 0 \
  "fix(dns): report a breaking-change note" $'fix: x\n\nThis is not a breaking change, just a mention of breaking change.\n' "$untouched" 0.2.39 "$guide_old_only"
check "an exclamation mark inside the title is not a marker" 0 \
  "fix(dns): stop the crash! now" $'fix(dns): stop the crash! now\n\nBody with a!: colon.\n' "$untouched" 0.2.39 "$guide_old_only"

check "a ! title with no guide change fails" 1 \
  "fix(dns)!: report the same records" "fix(dns): x"$'\n' "$untouched" 0.2.39 "$guide_02_to_03" \
  "doesn't change $guide_path"
check "a ! title with the guide changed but no section fails" 1 \
  "fix(dns)!: report the same records" "fix(dns): x"$'\n' "$touched" 0.2.39 "$guide_old_only" \
  "## 0.2 to 0.3"
check "a ! title with the guide changed and the section passes" 0 \
  "fix(dns)!: report the same records" "fix(dns): x"$'\n' "$touched" 0.2.39 "$guide_02_to_03" \
  "documented under \"## 0.2 to 0.3\""
check "a ! title without a scope is breaking" 1 \
  "feat!: drop the old flag" "feat: x"$'\n' "$untouched" 0.2.39 "$guide_02_to_03"
check "a section for the wrong line does not count" 1 \
  "fix(dns)!: report the same records" "fix(dns): x"$'\n' "$touched" 0.3.1 "$guide_02_to_03" \
  "## 0.3 to 0.4"
check "a section is found with trailing spaces on its heading" 0 \
  "fix(dns)!: x" "x"$'\n' "$touched" 0.2.39 $'# Upgrade\n\n## 0.2 to 0.3   \n\ntext\n'

check "a BREAKING CHANGE footer is breaking without a ! title" 1 \
  "fix(dns): report the same records" $'fix(dns): x\n\nBREAKING CHANGE: add no longer returns results.\n' "$untouched" 0.2.39 "$guide_02_to_03" \
  "BREAKING CHANGE:"
check "a BREAKING-CHANGE footer is breaking too" 1 \
  "fix(dns): report the same records" $'fix(dns): x\n\nBREAKING-CHANGE: add no longer returns results.\n' "$untouched" 0.2.39 "$guide_02_to_03"
check "a BREAKING CHANGE footer is satisfied by the guide" 0 \
  "fix(dns): report the same records" $'fix(dns): x\n\nBREAKING CHANGE: add no longer returns results.\n' "$touched" 0.2.39 "$guide_02_to_03"
check "a footer in a later commit still counts" 1 \
  "fix(dns): report the same records" $'fix: first\n\nfix: second\n\nBREAKING CHANGE: y\n' "$untouched" 0.2.39 "$guide_02_to_03" \
  "" $'fix: first\nfix: second'
check "a ! in a later commit's subject counts" 1 \
  "fix(dns): report the same records" $'fix: first\n\nfeat(dns)!: second\n' "$untouched" 0.2.39 "$guide_02_to_03" \
  "a commit subject has" $'fix: first\nfeat(dns)!: second'
check "a ! example in a commit body is not a marker" 0 \
  "docs: explain conventional commits" $'docs: explain conventional commits\n\nA breaking change looks like:\n\nfix!: remove the old flag\nfeat(dns)!: rename it\n' "$untouched" 0.2.39 "$guide_old_only" \
  "Not a breaking change"
check "a BREAKING CHANGE footer after a body example still counts" 1 \
  "docs: explain conventional commits" $'docs: explain\n\nfix!: example\n\nBREAKING CHANGE: y\n' "$untouched" 0.2.39 "$guide_02_to_03" \
  "a commit has a"
check "several markers are listed with separators" 1 \
  "fix(dns)!: x" $'fix(dns)!: x\n\nBREAKING CHANGE: y\n' "$untouched" 0.2.39 "$guide_02_to_03" \
  "before the colon; a commit subject"
check "a commit subject with a ! counts when the title has none" 1 \
  "fix(dns): report the same records" $'fix(dns): first\nfeat(dns)!: second\n' "$untouched" 0.2.39 "$guide_02_to_03" \
  "a commit subject has" $'fix(dns): first\nfeat(dns)!: second\n'
check "a lower-case footer is not a marker" 0 \
  "fix(dns): x" $'fix: x\n\nbreaking change: no\n' "$untouched" 0.2.39 "$guide_old_only"

check "from 1.0 on, the section is one major up" 0 \
  "feat(dns)!: x" "feat: x"$'\n' "$touched" 1.4.2 "$guide_1_to_2" \
  "## 1 to 2"
check "from 1.0 on, a missing section names the major line" 1 \
  "feat(dns)!: x" "feat: x"$'\n' "$touched" 1.4.2 "$guide_02_to_03" \
  "## 1 to 2"

action_only=$'action/action.yml\naction/install.sh\n'
check "a breaking change to only the action needs no CLI notes" 0 \
  "fix(action)!: rename the version input" "fix(action): x"$'\n' "$action_only" 0.2.39 "$guide_old_only" \
  "doesn't change anything under rust/"
check "a breaking footer on an action-only change needs no CLI notes" 0 \
  "fix(action): rename the version input" $'fix(action): x\n\nBREAKING CHANGE: the input is renamed.\n' "$action_only" 0.2.39 "$guide_old_only"
check "a breaking change touching both the action and the CLI needs CLI notes" 1 \
  "fix!: rename it everywhere" "fix: x"$'\n' $'action/action.yml\nrust/src/dns/add.rs\n' 0.2.39 "$guide_02_to_03" \
  "## 0.2 to 0.3"
check "a breaking change touching only docs and workflows needs no CLI notes" 0 \
  "ci!: change the runner" "ci: x"$'\n' $'.github/workflows/cicd.yml\ndocs/maintainers.md\n' 0.2.39 "$guide_old_only"
check "a directory that only starts like the CLI path does not count" 0 \
  "fix!: x" "fix: x"$'\n' $'rustic/notes.md\n' 0.2.39 "$guide_old_only"

# A guide with many sections is longer than a pipe buffer. `grep -q` stops reading at
# the first match, and `sed` then dies of SIGPIPE; under `pipefail` that once made a
# heading near the top look missing about three runs in four. Run it a few times,
# since a lucky run passes.
padding=
for _ in $(seq 1 2000); do
  padding+=$'A long line of guide text that pads the file out past the size of a pipe buffer.\n'
done
big_guide=$'# Upgrade\n\n## 0.1 to 0.2\n\ntext\n\n## 0.2 to 0.3\n\n'$padding
for attempt in 1 2 3 4 5 6; do
  check "a heading near the top of a long guide is found (run $attempt)" 0 \
    "fix(dns)!: x" "fix: x"$'\n' "$touched" 0.2.39 "$big_guide" \
    "documented under \"## 0.2 to 0.3\""
done

check "an unreadable version is an error" 2 \
  "fix(dns)!: x" "x"$'\n' "$touched" "not-a-version" "$guide_02_to_03" \
  "Can't read the crate version"

if [[ $failures -gt 0 ]]; then
  echo
  echo "$failures check(s) failed"
  exit 1
fi
echo
echo "All checks passed"
