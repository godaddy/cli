---
name: writing-guides
description: Use when you write or change a gddy guide (a rust/src/*/guides/*.md file). Covers where guides live, which rules apply, how to check a guide with Vale before opening a pull request, and the mistakes reviewers keep finding.
metadata:
  internal: true
---

# Write a guide

`gddy guide <name>` prints a guide as plain text in a terminal. Each guide is a Markdown file that the CLI embeds at build time.

## Where guides live

Guides sit next to the code for their command group, in `rust/src/<group>/guides/<name>.md`. For example, `rust/src/dns/guides/dns.md` is `gddy guide dns`. Go to [`docs/code-structure.md`](../../../docs/code-structure.md) for how source files mirror the command tree.

## Rules

The writing rules live in two places. Read them there, and don't copy them into a guide or a pull request.

- [`docs/command-authoring.md`](../../../docs/command-authoring.md), sections "Write for the customer" and "Guides".
- [`AGENTS.md`](../../../AGENTS.md), sections "User-Facing Text" and "Code File Structure".

Vale checks the mechanical rules (for example, contractions, headings, sentence length and em dashes). The rules are in `.vale/styles/GddyCli`, and `.vale.ini` switches them on for guides only.

## Check a guide before you open a pull request

CI runs Vale 3.24.0 (pinned in `.github/workflows/guides-lint.yml`). Use the same version locally. Go to the [Vale installation page](https://vale.sh/docs/vale-cli/installation/) to install it.

From the repository root:

```sh
vale sync
vale --glob='**/guides/*.md' rust/src
```

`vale sync` downloads the Microsoft package that `.vale.ini` lists. Run it once, and again when `.vale.ini` changes. The second command lints every guide. To lint one guide, pass its path: `vale rust/src/dns/guides/dns.md`. Vale exits with a nonzero code when it finds something.

Vale skips prose that follows a code block inside a list item. To catch it, also lint a copy of the guide with the list indentation removed. Copy `.vale` and `.vale.ini` into a temporary folder, because the settings in `.vale.ini` match on the path `rust/src/*/guides/`:

```sh
T=$(mktemp -d)
cp -r .vale .vale.ini "$T"/
for f in rust/src/*/guides/*.md; do
  mkdir -p "$T/$(dirname "$f")"
  sed -E 's/^[[:space:]]+//' "$f" > "$T/$f"
done
(cd "$T" && vale --glob='**/guides/*.md' rust/src)
```

Fix findings in the real file, not in the copy.

The check doesn't fail the pull request yet, but a reviewer will see the same findings. A guide that you touch should have none.

## Set up your editor

Install Vale locally (see above), then add the Vale extension to your editor so findings show as you type.

- **VS Code and editors based on it** (such as Cursor): install "Vale Linter" by Chris Chinchilla (`chrischinchilla.vale-vscode`) from the Marketplace. It needs Vale 3.10.0 or later. The older `errata-ai.vale-server` extension is deprecated.
- The extension finds `.vale.ini` at the repository root without any setting. To point it there explicitly, add this to `.vscode/settings.json` (the repository ignores that file) or to your user settings:

  ```json
  { "vale.valeCLI.config": "${workspaceFolder}/.vale.ini" }
  ```

- If `vale` isn't on your `PATH`, set `vale.valeCLI.path` to the binary.
- Run **Vale: Sync** from the command palette once, so the Microsoft package is available.

Go to the [extension's README](https://github.com/ChrisChinchilla/vale-vscode#readme) for the full list of settings. Other editors: go to the [Vale documentation](https://vale.sh/docs/) for a plugin.

## Mistakes reviewers keep finding

- **No links within the guide.** A terminal shows `[text](#section)` as raw text and can't jump. Name the section in quotes and say whether it comes earlier or later: `go to the "Buy a plan" section later in this guide`. A link to another guide or a web page is fine.
- **"Go to", not "See".** Write "Go to X for Y". End the guide with a "Related" section, not "See also".
- **Concepts format.** Put a "Concepts" section first, and give each term one line: `**Term**: definition.` Explain what the thing is there. Put behavior (what a command does, how long it takes) in the how-to section for that command.
- **Verb-first headings.** Use "Create the site", not "Creating a site" or "Site creation". No numbered headings.
- **Lead-ins.** Put a sentence between a heading and its first sub-heading.
- **Soft line breaks.** One paragraph is one line. Hard breaks belong only in shell examples.
- **`AI assistants:` notes carry consent guidance.** Vale skips them, so don't reword them to fix style findings. The same text sits in Rust constants and next-action descriptions, and tests assert parts of that Rust text. If command behavior changes, update the guide note and the Rust text together, keep the consent meaning, and run the related tests.
- **Rust tests pin other guide strings.** Before you change a sentence or a heading, search `rust/` for it (for example `rg -F "What the CLI doesn't do" rust`). A test that pins the old wording fails in CI.
- **Check each claim against the code.** Read the command's source, or run it, before you describe its behavior, error codes or timing.
- **No internal names.** This repository is public. Leave out ticket keys, internal system and service names, Slack channels, and internal URLs in guides, commits and pull request text.
- **Sweep the whole guide.** When a fix applies to one heading or phrase (a gerund, a term), search the rest of the guide and the other guides for the same pattern.
