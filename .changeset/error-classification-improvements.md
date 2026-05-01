---
"@godaddy/cli": minor
---

Improve error classification for application lifecycle commands (`create`, `update`, `archive`, deploy upload). Server-side failures previously collapsed into a generic `NETWORK_ERROR` envelope now surface with an accurate top-level `code` (`VALIDATION_ERROR`, `FORBIDDEN`, `NOT_FOUND`, `CONFLICT`, `RATE_LIMIT_EXCEEDED`, `AUTH_REQUIRED`) and a specific, actionable `fix` string. Per-field validation details are forwarded under `error.details.fields` so consumers can render or react to individual field problems. Client-side input validation (rejected by the CLI before sending) remains distinct from server-side validation errors. Unknown server error codes and transport failures still map to `NETWORK_ERROR`, but now preserve the full HTTP context (status, response body, original error code) under `error.details.response` so agents can discriminate further when needed.
