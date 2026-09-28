# gddy skill

Teaches an AI coding agent how to drive `gddy`, GoDaddy's CLI for:

- domain search/registration
- DNS management
- hosting
- email
- product purchasing
- the GoDaddy Developer Platform

See [SKILL.md](./SKILL.md) for the full instructions given to the agent.

## Installation

### Claude Code

```bash
claude plugin marketplace add godaddy/cli
claude plugin install gddy@godaddy
```

### Any other AI coding agent

This repo is also compatible with [skills](https://github.com/vercel-labs/skills), a package-manager-style installer for agent skills that isn't tied to Claude Code — it supports Cursor, Codex, Windsurf, opencode, and 70+ other agents in addition to Claude Code:

```bash
npx skills add godaddy/cli --skill gddy --agent <agent>
```

Swap `<agent>` for whichever agent you use (`claude-code`, `cursor`, `codex`, `windsurf`, `opencode`, ...). Run `npx skills add --help` for the full list of supported agents.
