---
name: breaking-changes
description: Use when a change to gddy could break a user's existing scripts or settings, such as a renamed or removed flag or command, a changed JSON output shape or exit code, a renamed environment variable, or a changed config file. Documents the change in the upgrade guide and marks the commit and pull request as breaking.
metadata:
  internal: true
---

# Document breaking changes in the upgrade guide

`gddy guide upgrades` (source: `rust/src/update/guides/upgrades.md`) tells users what to change when they upgrade across a breaking release. `gddy update apply` points users to it when an update crosses one. A breaking change isn't finished until the guide covers it.

## What counts as breaking

A change is breaking when a script, CI job or saved setting that worked before can fail, or can quietly do something different, after the upgrade. Examples:

- A flag, command or environment variable that is removed, renamed, or changes meaning.
- A JSON field that is removed, renamed, moved, or changes type, in any command's output (`--output json` and the default fields).
- A changed `action` or status string that scripts might compare against.
- A changed error code, or an error where there used to be a silent fallback.
- A changed config file name, location or format, or a stricter check on one.
- A command that now needs a login, a scope or a flag it didn't before.

Adding a field, a command or an optional flag isn't breaking. When unsure, treat the change as breaking: an extra section costs little, and a silent break costs a customer's pipeline.

## Steps

1. **Mark the commit and the PR title as breaking.** Put `!` after the type or scope (`fix(dns)!: ...`) and add a `BREAKING CHANGE:` footer to the commit message. The footer text goes into the release notes, so write it for a user: what changed and what to do. Release-please bumps the version past the breaking line only because of this marker.
2. **Find the section to write.** Read the version in `rust/Cargo.toml`. Before 1.0, a breaking release moves the second number (`0.2.39` upgrades to `0.3.0`, so the section is `## 0.2 to 0.3`). From 1.0 on, it moves the first (`## 1 to 2`). If the section for the next line already exists because an earlier unreleased change added it, add to it. Don't start a second one.
3. **Write what the user has to change.** For each affected command, flag, variable or field, say what it was, what it is now, who is affected, and what to do. Use a before and after table for renamed or moved things, and show a short example when the new form isn't obvious. Say when an old setting is ignored without an error, since that's the case users can't notice on their own.
4. **Check every claim.** Run the old release to see the old behavior (`git worktree add /tmp/old v<version>`, then build it), or read its source at that tag. Run the new behavior from your branch. Don't describe behavior from memory or from the changelog alone.
5. **Follow the guide style** in `docs/command-authoring.md` and `AGENTS.md`: write for the customer, one line per paragraph, and run `vale --config .vale.ini rust/src/update/guides/upgrades.md` before pushing.
6. **Say so in the PR description.** Summarize the break and link to the section. The description must match what the code does.

## The checks that enforce this

The `Breaking change docs` workflow (`.github/workflows/breaking-docs.yml`, script `.github/scripts/check-breaking-docs.sh`) fails a pull request whose title or commits mark a break, with a `!` or a `BREAKING CHANGE:` footer, unless the pull request changes `upgrades.md` and the guide has the `## <from> to <to>` section for the upgrade it ships in. It re-runs when you edit the title. To test it locally, run `bash .github/scripts/check-breaking-docs-test.sh`.

A second check backs it up at release time. A test in `rust/src/update/breaking.rs` fails when the version in `Cargo.toml` is in a breaking line that has no section titled `## <previous> to <this line>`. Release-please raises the version in its release PR, so that PR fails CI until the section exists. Write the section in the same PR as the breaking change, not afterward.
