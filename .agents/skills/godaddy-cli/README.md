# `godaddy-cli` skill

Teaches an AI coding agent how to drive the `godaddy` CLI — applications, auth, environments, releases/deploys, extensions, and webhooks on the GoDaddy Developer Platform. It covers the JSON output contract, `next_actions` discovery, error codes, and the typical create/deploy/diagnose workflows, so the agent doesn't have to reverse-engineer any of that from `--help` output alone.

See [SKILL.md](./SKILL.md) for the full instructions given to the agent.

## Installation

### Claude Code

```bash
claude plugin marketplace add godaddy/cli
claude plugin install godaddy-cli@godaddy
```

This installs the plugin that bundles both skills in this repo (`godaddy-cli` and `gddy`); Claude picks whichever one fits the task automatically.

### Any other AI coding agent

This repo is also compatible with [`skills`](https://github.com/vercel-labs/skills), a package-manager-style installer for agent skills that isn't tied to Claude Code — it supports Cursor, Codex, Windsurf, opencode, and 70+ other agents in addition to Claude Code:

```bash
npx skills add godaddy/cli --skill godaddy-cli --agent claude-code
```

Swap `--agent claude-code` for whichever agent you use (`cursor`, `codex`, `windsurf`, `opencode`, ...). Run `npx skills add --help` for the full list of supported agents.

## What it doesn't cover

Domain search, registration, and DNS management live in a separate tool, `gddy`, with its own skill — see [`../gddy/README.md`](../gddy/README.md).
