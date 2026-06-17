---
"@godaddy/cli": patch
---

Default the unset CLI environment to production so `godaddy auth login` uses the production OAuth flow unless `--env ote` or `godaddy env set ote` is selected.
