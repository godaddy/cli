---
"@godaddy/cli": minor
---

Add terminal-based agreement acceptance and automated onboarding to `auth login`

- Agreement prompt (ToS, Privacy Policy, Developer Agreement) is shown on stderr before completing onboarding — only for new users whose org is PENDING, never for returning users
- Onboarding completes automatically after OAuth via a single `POST /api/v1/onboarding/cli` call; no browser redirect to the portal required
- Added `--accept-agreements` flag for non-interactive/CI use; without it, non-TTY runs with a PENDING org emit a structured `AGREEMENTS_REQUIRED` error (`ok: false`, exit 1)
- `org_id` and `onboarding` status are included in the login result envelope
