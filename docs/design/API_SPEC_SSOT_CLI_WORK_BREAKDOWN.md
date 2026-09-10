# Work Breakdown: CLI API Spec Pin + Sync (Domains pilot first)

**Repo focus:** `godaddy/cli`  
**Pilot upstream:** [`gdcorp-platform/domains.domain-lifecycle-specification`](https://github.com/gdcorp-platform/domains.domain-lifecycle-specification)  
**Related design:** [walkthrough](./API_SPEC_SSOT_IMPLEMENTATION_WALKTHROUGH.md) · [downstream sync](./API_SPEC_SSOT_DOWNSTREAM_SYNC.md) · [architect one-pager](./API_SPEC_SSOT_ARCHITECT_ONEPAGER.md)

**Goal:** Stop hand-vendoring Domains (and later other) OpenAPI into the CLI. Each `*-specification` release notifies `godaddy/cli`; a workflow pulls the artifact, bumps a pin, regenerates only that slice, and opens a PR.

```text
domains.domain-lifecycle-specification
  → GitHub Release + artifact
  → repository_dispatch → godaddy/cli
  → pin + update OpenAPI path + codegen/catalog regen
  → PR → merge → next CLI release
```

**Out of scope for this WBS:** consolidating into `api-spec` monorepo; rewriting portal MDX (docs has a parallel track). `api-spec` stays review/legacy unless an API’s only source is still there.

---

## Current CLI state (why this work exists)

| Artifact | Path today | Problem |
|----------|------------|---------|
| Domains OpenAPI (progenitor SoT) | `rust/domains-client/openapi/domains.oas3.json` | Manually curated / vendored; build is hermetic |
| Hosting OpenAPI | `rust/schemas/openapi/hosting-nodejs-public-v1.yaml` | Vendored local |
| Catalog sources | `rust/api-catalog-sources.json` | Remotes clone `*-specification` tips; Domains/hosting forced **local** |
| Generated catalog | `rust/schemas/api/*.json` | Can drift from upstream without a pin |

Domains is the right pilot: highest CLI usage, clear upstream repo, already a second copy next to the portal’s `domains-v3` tree.

---

## Phase C0 — Decisions & inventory (CLI + Domains) — 2–4 days

| ID | Task | Owner | Done when |
|----|------|-------|-----------|
| C0.1 | Confirm `packageId` for Domains Lifecycle = `domains/v3` (same id docs will use) | CLI + Domains + Platform | Written in pin + notify payload |
| C0.2 | Decide CLI pin file path: recommend `rust/schemas/api/pins.json` (schema = walkthrough §5.2) | CLI | Path agreed |
| C0.3 | Decide CLI package-map (or embed in pins / small `api-spec-package-map.json`): `domains/v3` → OpenAPI path(s) + regen commands | CLI | Map schema agreed |
| C0.4 | Document how Domains tarball maps into CLI layout (full tree vs trimmed `domains.oas3.json` used by progenitor) | CLI + Domains | Note in CONTRIBUTING / this doc |
| C0.5 | List which CLI consumers of Domains OpenAPI must regen: `domains-client` build, optional catalog domain `domains` | CLI | Checklist |
| C0.6 | Confirm notify consumers for Domains release: at least `godaddy/cli` (docs can be parallel) | Platform | Domains release job will call CLI |

**Exit:** Pilot contract signed: one `packageId`, one pin path, one OpenAPI landing path, regen steps listed.

### Domains OpenAPI shape (important)

Upstream lives under `v3/schemas/` (multi-file + `common-types` submodule). CLI today uses a **merged/trimmed** `domains.oas3.json` for progenitor.

Pick one for the pilot:

| Option | Pros | Cons |
|--------|------|------|
| **A. Pin stores full release tree; script produces `domains.oas3.json`** (recommended) | Matches SSOT bytes; trim stays a CLI concern | Need a stable `scripts/materialize-domains-client-spec.sh` |
| **B. Release already includes a CLI-ready bundled JSON** | Simpler CLI job | Couples Domains release packaging to CLI |

**Recommendation:** Option A for pilot — Domains publishes the normal OpenAPI artifact; CLI materializes the progenitor input in the sync job.

---

## Phase C1 — Domains `*-specification` publisher (blocking for CLI) — 1–3 weeks

Work in [`domains.domain-lifecycle-specification`](https://github.com/gdcorp-platform/domains.domain-lifecycle-specification) (and shared template). **Not** in `api-spec`.

| ID | Task | Repo | Done when |
|----|------|------|-----------|
| C1.1 | Spectral (+ oasdiff vs previous release) on PRs touching `v3/schemas/**` | Domains spec | Breaking change fails CI without process |
| C1.2 | Version bump enforcement when OpenAPI changes | Domains spec | PR without version bump fails |
| C1.3 | Release workflow: bundle/dereference OpenAPI → GitHub Release asset + `sha256` + `manifest.json` | Domains spec | Tag e.g. `v1.4.2` has downloadable tarball |
| C1.4 | Notify step: `repository_dispatch` to `godaddy/cli` with `packageId`, `packageVersion`, `tag`, `assetUrl`, `sha256`, `sourceRepo` | Domains spec | CLI workflow can be triggered (even if no-op at first) |
| C1.5 | Optional: also notify docs repo (same payload) if portal track is ready | Domains spec | Dual notify |
| C1.6 | **After Domains works:** extract reusable `release-and-notify.yml` into template; Domains becomes a thin `uses:` caller | `api-specification-template` + Domains | Template hosts reusable WF; Domains still green |
| C1.7 | Document release runbook in Domains README (how to cut a release) | Domains spec | Linked from CLI sync docs |

**Sequencing (recommended):** implement **C1.3–C1.4 as a normal (non-reusable) workflow in Domains first**. Do **not** block the pilot on the template. Promote to `api-specification-template` only in **C1.6** once release + notify are proven end-to-end with CLI. `release-and-notify.yml` does **not** exist in the template today (template has `validate.yml` only).

**Pilot sketch (Domains-local, C1.3–C1.4):**

```yaml
# .github/workflows/release.yml  — live in Domains until C1.6
name: Release OpenAPI and notify consumers
on:
  push:
    branches: [main, develop]  # match repo default
    paths: ["v3/schemas/**", ".github/workflows/release.yml"]
  workflow_dispatch:

jobs:
  release:
    runs-on: ubuntu-latest
    steps:
      # bundle OpenAPI → GitHub Release asset + sha256 + manifest
      # repository_dispatch → godaddy/cli (packageId=domains/v3, …)
```

**End-state sketch (after C1.6):**

```yaml
# Domains: thin caller only
jobs:
  release:
    uses: gdcorp-platform/api-specification-template/.github/workflows/release-and-notify.yml@develop
    with:
      package_id: domains/v3
    secrets: inherit
```

**Secrets / App:** PAT or GitHub App that can `repository_dispatch` on `godaddy/cli` (cross-org).

**Exit:** Merging a Domains OpenAPI change produces a release artifact **and** a dispatch event CLI can receive.

**`api-spec` in this phase:** no change required for Domains v3. Only if Architecture still requires a parallel review PR, document that as a process link — it is **not** the bytes the CLI pins.

---

## Phase C2 — CLI pin + map + materialize (foundation) — 1–2 weeks

Work in `godaddy/cli`.

| ID | Task | Done when |
|----|------|-----------|
| C2.1 | Add `rust/schemas/api/pins.json` with initial `domains/v3` entry matching **current** vendored `domains.oas3.json` (or “unknown” until first sync) | File committed; §5.2-valid |
| C2.2 | Add `rust/schemas/api/package-map.json` (or equivalent): |  |
| | `domains/v3` → `{ "openapiPath": "domains-client/openapi/…", "materializeScript": "…", "regen": ["domains-client", "catalog?"] }` | Lookup works |
| C2.3 | Script: download release asset, verify `sha256`, extract | Unit/integration test with fixture tarball |
| C2.4 | Script: materialize progenitor input (`domains.oas3.json`) from extracted tree (Option A) | Output matches (or deliberately documents) trim rules |
| C2.5 | Script: update `pins.json` for one `packageId` | Idempotent |
| C2.6 | Document local dry-run: `./scripts/sync-api-spec.sh --packageId domains/v3 --tag vX.Y.Z` | README / `docs/` note |
| C2.7 | CI: optional `pins` schema validate on PR | Check passes |

**Exit:** An engineer can manually sync Domains from a release tag without editing OpenAPI by hand.

---

## Phase C3 — CLI codegen / catalog regen gates — 1–2 weeks

| ID | Task | Done when |
|----|------|-----------|
| C3.1 | After materialize: ensure `domains-client` builds (`cargo check -p domains-client`) | Green |
| C3.2 | If catalog should track Domains from pin: update `generate-api-catalog` / `api-catalog-sources.json` so `domains` is **pin-driven local path**, not ad-hoc tip clone | Catalog regen uses pinned file |
| C3.3 | Regen-check CI: if OpenAPI/pin change without matching generated artifacts → fail (or require sync script output committed) | Drift fails CI |
| C3.4 | Clarify what is committed: OpenAPI JSON always; progenitor output is build-time (keep hermetic `build.rs`) | Documented |
| C3.5 | Tests: domain commands still pass against fixtures / recorded behavior after a sample sync | `cargo test` green |

**Note:** progenitor runs at **compile** time from committed `domains.oas3.json`. The sync PR’s main committed delta is usually **pin + OpenAPI JSON** (+ catalog JSON if applicable), not checked-in Rust from progenitor.

**Exit:** Pin bump + OpenAPI update is enough for CI to prove the client still builds/tests.

---

## Phase C4 — CLI GitHub Action (automated PR) — 1–2 weeks

| ID | Task | Done when |
|----|------|-----------|
| C4.1 | Add `.github/workflows/sync-api-spec.yml` on `repository_dispatch` (`api-spec-release`) + `workflow_dispatch` | Workflow exists |
| C4.2 | Job: read payload → map → download/verify → materialize → update pin → regen catalog if needed → open PR | PR opened on real Domains dispatch |
| C4.3 | PR title/body: `chore(api-spec): bump domains/v3 to x.y.z` + sourceRepo, tag, sha256 | Consistent with docs bot |
| C4.4 | Labels / CODEOWNERS for sync PRs | Review path clear |
| C4.5 | Auto-merge policy (optional): patch only, or none for CLI | Documented |
| C4.6 | E2E with Domains: cut test release → CLI PR appears within ~1 hour | Demo recorded |
| C4.7 | Failure Slack/email if sync job fails | Alert exists |

**Workflow permissions:** `contents: write`, `pull-requests: write` (or GitHub App).

**Exit:** Domains release automatically opens a CLI sync PR; human merges.

---

## Phase C5 — Hardening & CLI docs — ~1 week

| ID | Task | Done when |
|----|------|-----------|
| C5.1 | Weekly drift job: compare `pins.json` to latest Domains release; open/update PR or issue | Drift visible |
| C5.2 | Branch protection / PR template: discourage hand-edits to pinned OpenAPI without pin bump | Policy noted |
| C5.3 | Update AGENTS.md / contributor docs: “ Domains OpenAPI comes from pin sync” | Linked |
| C5.4 | Align `api-catalog-sources.json` rationale text with pin model | Comment accurate |
| C5.5 | Optional: show pin version in `gddy --version` / debug metadata | Nice-to-have |

---

## Phase C6 — Next APIs after Domains (only when needed) — ongoing

Do **not** block Domains pilot on full estate.

| ID | Task | Notes |
|----|------|-------|
| C6.1 | Add release+notify to next `*-specification` CLI cares about (e.g. hosting if still local, or high-use commerce catalog domains) | Reuse template caller; scripted backport later |
| C6.2 | Extend CLI `package-map.json` + pin keys | One key per API |
| C6.3 | For catalog **remote** domains: prefer pin+artifact over cloning default branch tip | Reduces tip drift |
| C6.4 | Org backport script for many `*-specification` callers | Walkthrough §1.2; after Domains+1 proven |

**`api-spec`:** still not the Domains write path. Only onboard if a CLI-used API’s editable OpenAPI still lives only there.

---

## Suggested sequence (you start with Domains)

```text
Week 1–2   C0 + C1.1–C1.4 (Domains can release + notify)
Week 2–4   C2 + C3 (CLI can sync manually + CI gates)
Week 4–5   C4 (automated PR)
Week 5–6   C5 + first real Domains→CLI sync merged
Later      C6 (more APIs), docs portal parallel track
```

**Parallel:** Docs portal sync (walkthrough Phase 2) can use the **same** Domains notify payload; not required to finish CLI pilot.

---

## Cross-repo checklist (Domains pilot)

### `domains.domain-lifecycle-specification`
- [ ] Lint / breaking-change CI  
- [ ] Release artifact + checksum  
- [ ] Notify `godaddy/cli` (`packageId=domains/v3`)  
- [ ] Thin caller → reusable workflow (template)

### `api-specification-template`
- [ ] Reusable `release-and-notify.yml`  
- [ ] Document `package_id` + secrets  

### `godaddy/cli`
- [ ] `pins.json` + package map  
- [ ] Download / materialize / pin scripts  
- [ ] `sync-api-spec.yml` → PR  
- [ ] Regen-check CI  
- [ ] Domains E2E from a real tag  

### `api-spec`
- [ ] No Domains v3 pin source  
- [ ] Optional: link/process note only if Architecture still requires review PRs there  

### `developer-ecosystem-documentation` (optional parallel)
- [ ] Same dispatch; separate pin + generate MDX  

---

## Verification (Domains pilot done)

1. Tag a Domains release (or `workflow_dispatch` release).  
2. CLI receives `api-spec-release` for `domains/v3`.  
3. Bot PR updates `pins.json` + Domains OpenAPI path; `cargo check -p domains-client` and tests pass on the PR.  
4. Merge PR; local `cargo build` uses new pin without network.  
5. A second API’s pin is unchanged.

---

## Effort snapshot

| Phase | Rough effort | Critical path? |
|-------|--------------|----------------|
| C0 Inventory | 2–4 days | Yes |
| C1 Domains publisher | 1–3 weeks | **Yes — start here** |
| C2 CLI pin/scripts | 1–2 weeks | Yes (after C1.3+) |
| C3 Regen gates | 1–2 weeks | Yes |
| C4 Automation PR | 1–2 weeks | Yes |
| C5 Hardening | ~1 week | No |
| C6 More APIs | Ongoing | No |

**Pilot total (Domains → CLI automated PR):** ~6–10 weeks calendar, depending on App/secrets and materialize-script complexity.

---

## References

- Walkthrough §1–§5 (notify, pins schema, package map), §8 phases  
- CLI: `rust/api-catalog-sources.json`, `rust/domains-client/openapi/`, `rust/tools/generate-api-catalog/`  
- Upstream: [domains.domain-lifecycle-specification](https://github.com/gdcorp-platform/domains.domain-lifecycle-specification)  
- Template: [api-specification-template](https://github.com/gdcorp-platform/api-specification-template)  
