# gddy skill

Teaches an AI coding agent how to drive `gddy`, GoDaddy's beta CLI for domain search, registration, and DNS management. It covers install/auth, the two-step quote-then-purchase flow for buying a domain, DNS record semantics (`add` vs `set` vs `delete`), and how to use the CLI's own self-documentation (`--help`, `--search`, `tree`, `guide`) to stay current as `gddy` evolves.

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
npx skills add godaddy/cli --skill gddy --agent claude-code
```

Swap `--agent claude-code` for whichever agent you use (`cursor`, `codex`, `windsurf`, `opencode`, ...). Run `npx skills add --help` for the full list of supported agents.
