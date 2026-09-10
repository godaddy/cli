# API Spec SSOT + Downstream Sync — Architect One-Pager

**Audience:** Architecture / API Design / Platform  
**Status:** Proposed (design docs on `wip-docs`)  
**Pilot API:** Domains Lifecycle v3  
**Date:** September 2026

---

## Problem (today)

| Issue | What happens |
|-------|----------------|
| **Multiple editable copies** | Modern OpenAPI lives in `*-specification` repos; the developer portal and CLI hold **vendored** copies updated by hand. |
| **Drift** | Example: Domains Lifecycle tip (Aug 28) ahead of last portal sync (Aug 14) — public docs can lag the real contract. |
| **Unclear source of truth** | `api-spec` is the **architecture review + legacy Swagger catalog**; Domains v3 is **not** authored there. Easy to assume one repo feeds everything. |
| **No versioned handoff** | Consumers chase git tips or Slack copies instead of immutable, pinned releases. |
| **Cross-org friction** | Specs in `gdcorp-platform`, docs in `gdcorp-commerce`, CLI in `godaddy` — manual sync does not scale. |

**Failure modes:** wrong docs for partners/agents, CLI/codegen out of date, review process disconnected from what actually ships on [developer.godaddy.com](https://developer.godaddy.com/en/docs/references/rest/domains/v3).

---

## Solution (proposed)

**Keep federated SSOT** — do **not** migrate into an `api-spec` monorepo.

```
*-specification repo     →  release OpenAPI artifact
 (e.g. Domains Lifecycle)    →  notify docs + CLI (hardcoded, <3 repos)
                             →  consumer pulls artifact, bumps pin, opens PR
                             →  humans / auto-merge → portal & CLI update
```

| Piece | Role |
|-------|------|
| `gdcorp-platform/<domain>.<capability>-specification` | **Editable SSOT** (from [`api-specification-template`](https://github.com/gdcorp-platform/api-specification-template)) |
| Release + notify workflow (reusable) | Publish versioned OpenAPI; `repository_dispatch` to consumers |
| `developer-ecosystem-documentation` | Pin + vendor + generate — **bot PRs only** for specs |
| `godaddy/cli` | Pin + codegen — same notify pattern |
| `api-spec` | **Review/legacy only** — exposure-based Architecture Review; not Domains v3 authoring |

**Rule:** one public API version → **one** upstream repo → **one** pin. Never copy Domains v3 from both `api-spec` and the lifecycle repo.

**Sync model:** push notify + pull artifact + **opt-in pin PR** (Dependabot-style). Deterministic: payload carries version, asset URL, checksum — replace that path only (OpenAPI in, OpenAPI out; no Swagger conversion).

---

## Why this fits our use case

| Criterion | Why federated + pin PRs wins |
|-----------|------------------------------|
| **Already how we create APIs** | ~80 `*-specification` repos from the GitHub template; isolation is free. |
| **Timeline** | Automate what we have — no multi-quarter monorepo migration. |
| **Blast radius** | Domains release never republishes Shoppers; only that pin moves. |
| **Cross-org** | Works with a small hardcoded consumer list (docs + CLI) and one App/token. |
| **Governance** | `api-spec` review process can continue for legacy/Published policy without being the write path for modern OAS. |
| **Reproducibility** | Pins answer “what is the portal/CLI on?”; weekly drift catches missed webhooks. |

**Rejected for now:** consolidating all specs under `api-spec/apis/…` — higher cost, weaker fit to current ownership, same consumer UX achievable without the move.

---

## Timeline (pilot → scale)

| Phase | Focus | Duration |
|-------|--------|----------|
| **0** | Policy, inventory (portal key → single upstream), one-pin rule | 1–2 weeks |
| **1** | Domains: Spectral/oasdiff, release artifact, notify docs+CLI; reusable workflow in template | 2–4 weeks |
| **2** | Docs: `pins.json`, sync workflow, branch protection, E2E portal PR | 2–4 weeks |
| **3** | CLI: pin + regen-check on same dispatch | 1–3 weeks |
| **4** | **Backport** thin release+notify caller onto more **existing** `*-specification` repos via **scripted PRs** (template does not auto-update old repos); drift reports | Ongoing |
| **5** | Service contract tests against pinned artifact | Later / parallel |

**Pilot (Domains → portal + CLI):** ~6–10 weeks.  
**Existing estate:** ~80 repos need the caller only when they should drive
docs/CLI — prioritize portal-backed APIs; not a big-bang on day one.  
**New repos:** inherit caller from the updated template.

---

## Asks of Architecture

1. **Endorse** federated `*-specification` as SSOT for modern Published APIs; `api-spec` remains review/legacy.
2. **Endorse** automated pin PRs to docs + CLI (no hand-edited OpenAPI in the portal for pinned APIs).
3. **Clarify** (open): for Published APIs, is an `api-spec` PR still required *in addition to* the specification repo, or does review shift?
4. **Sponsor** cross-org GitHub App / token for notify + consumer PRs.

---

## Design references (detail)

- [API_SPEC_SSOT_DOWNSTREAM_SYNC.md](./API_SPEC_SSOT_DOWNSTREAM_SYNC.md) — architecture + WBS  
- [API_SPEC_SSOT_IMPLEMENTATION_WALKTHROUGH.md](./API_SPEC_SSOT_IMPLEMENTATION_WALKTHROUGH.md) — end-to-end flow  
- [API_SPEC_SSOT_CLI_WORK_BREAKDOWN.md](./API_SPEC_SSOT_CLI_WORK_BREAKDOWN.md) — CLI + Domains pilot tasks  
- [API_SPEC_REPO_ANALYSIS_SSOT.md](./API_SPEC_REPO_ANALYSIS_SSOT.md) — live repo mapping  

**One line:** Capability teams own OpenAPI in their `*-specification` repo; releases notify the developer portal and CLI; those repos pin and sync through automation — never hand-copied contracts, never two sources for one API.
