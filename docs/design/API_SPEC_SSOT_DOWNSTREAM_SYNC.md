# Design: API Spec Single Source of Truth + Downstream Sync

---

## Problem Statement

GoDaddy's developer platform API specifications currently flow through multiple repositories and organizations.

> **CLI work breakdown (Domains pilot first):**  
> [API_SPEC_SSOT_CLI_WORK_BREAKDOWN.md](./API_SPEC_SSOT_CLI_WORK_BREAKDOWN.md).
>
> **Architect one-pager:** shareable summary (problem, solution, why, timeline) in
> [API_SPEC_SSOT_ARCHITECT_ONEPAGER.md](./API_SPEC_SSOT_ARCHITECT_ONEPAGER.md).
>
> **Implementation walkthrough:** step-by-step publish → pin → consumer PR flow
> (push vs pull, portal sync, how teams learn about new versions) is in
> [API_SPEC_SSOT_IMPLEMENTATION_WALKTHROUGH.md](./API_SPEC_SSOT_IMPLEMENTATION_WALKTHROUGH.md).
>
> **Repo reality check (Aug 2026):** A full analysis of the live repos is in
> [API_SPEC_REPO_ANALYSIS_SSOT.md](./API_SPEC_REPO_ANALYSIS_SSOT.md). Key correction:
> modern Published APIs (e.g. Domains Lifecycle v3) are authored in
> `gdcorp-platform/<domain>.<capability>-specification` repos, **not** solely in
> `api-spec`. `api-spec` remains the architecture-review / legacy registry.
> `developer-ecosystem-documentation` vendors copies under `openapi-specs/specs/`
> and generates reference MDX — sync today is mostly manual.

```
gdcorp-platform/<domain>.<capability>-specification   ◄── modern SSOT (per API)
gdcorp-platform/api-spec                              ◄── review + legacy Swagger
        │
        ▼  (manual / ad-hoc vendoring today)
gdcorp-commerce/developer-ecosystem-documentation
        │   openapi-specs/specs/ + registry.ts + npm run generate
        ▼
Team service repos (different orgs) + consumers (gddy CLI, SDKs)
```

This creates two independent failure modes:

1. **File drift** — the same API is represented in multiple places and copies diverge over time.
2. **Implementation drift** — a service's runtime behavior no longer matches what any published spec says.

Manual sync does not scale. Teams can merge implementation changes that break the public contract without anyone noticing until a consumer fails in production.

## Goals

- Keep **one editable source per API capability** in its existing
  `gdcorp-platform/<domain>.<capability>-specification` repo.
- **Automate propagation** to developer docs and consumer repos — no hand-editing downstream copies.
- **Block breaking changes** at PR time unless explicitly versioned and approved.
- **Prove implementation matches spec** before service releases (phased rollout).
- Work across **org boundaries** via versioned artifacts (GitHub Releases, npm package, or internal registry).

## Non-Goals

- Replacing Pact/consumer-driven contracts for all APIs in phase 1 (optional later layer).
- Mandating Backstage/catalog setup before SSOT is working.
- Consolidating all team implementation repos into one org.
- **Migrating all `*-specification` repos into an `api-spec` monorepo** (rejected for
  this design — keep federated repos; see walkthrough).

---

## Recommended Architecture

### Principle: Write once, publish many — **per specification repo**

Canonical edits happen in the capability’s **`*-specification` repository**
(created from [`api-specification-template`](https://github.com/gdcorp-platform/api-specification-template)).
Example: Domains Lifecycle v3 lives in
`domains.domain-lifecycle-specification` under `v3/schemas/`.

Each repo publishes **its own** versioned OpenAPI artifact. A Domains release
does **not** bump Shoppers or any other API. After publish, that repo’s GitHub
workflow **notifies a hardcoded list** of consumers (&lt;3: developer portal,
`godaddy/cli`, optional service). Consumers pin and open sync PRs.

```
┌─────────────────────────────────────────────────────────────────┐
│  gdcorp-platform/<domain>.<capability>-specification            │
│  (SSOT — humans edit here; one repo per capability)             │
│  Example: domains.domain-lifecycle-specification/v3/schemas/    │
│  • Spectral lint + oasdiff on every PR                          │
│  • Release workflow → artifact + tag                            │
│  • Notify hardcoded consumers (reusable workflow / template)    │
│                                                                 │
│  gdcorp-platform/api-spec  (review gate + legacy Swagger store) │
└──────────────────────────┬──────────────────────────────────────┘
                           │ merge → GitHub Release for THIS api only
                           │ push: repository_dispatch → docs, cli, …
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│  Consumer sync workflows (one per consumer repo)                │
│  • Pull exact artifact (checksum)                               │
│  • Update pins.json + that API’s vendored/codegen slice only    │
│  • Open PR (auto-merge patch/minor optional)                    │
└──────────────────────────┬──────────────────────────────────────┘
                           │
           ┌───────────────┼───────────────┐
           ▼               ▼               ▼
   ┌──────────────┐ ┌──────────────┐ ┌──────────────────────┐
   │ docs portal  │ │ service repos│ │ godaddy/cli          │
   │ pin + generate│ │ (pin+verify) │ │ pin + codegen        │
   └──────────────┘ └──────────────┘ └──────────────────────┘
```

> **Implementation detail:** see
> [API_SPEC_SSOT_IMPLEMENTATION_WALKTHROUGH.md](./API_SPEC_SSOT_IMPLEMENTATION_WALKTHROUGH.md).
> Each `*-specification` needs publish+notify (share via reusable workflow);
> docs/CLI each need one sync handler keyed by `packageId`.

### Repository roles

| Repository | Role | Who edits | Sync mechanism |
|------------|------|-----------|----------------|
| `gdcorp-platform/<domain>.<capability>-specification` | **Canonical contract** | Capability team (via PR) | Release + notify hardcoded consumers |
| `gdcorp-platform/api-specification-template` | Bootstrap + reusable release/notify workflow | Platform | New repos inherit caller workflow |
| `gdcorp-platform/api-spec` | **Review / legacy registry** | API owners via architecture review | Not the Domains v3 write path |
| `gdcorp-platform/api-specification-aggregator` | **Discovery view** | Submodule pins only | Manual submodule add for new repos |
| `gdcorp-commerce/developer-ecosystem-documentation` | **Published developer docs** | Guides: humans; `openapi-specs/specs/`: bot only | `repository_dispatch` → pin PR |
| Team service repos | **Implementation** | Service teams | Pin package; CI verifies |
| `godaddy/cli` | **CLI API catalog / clients** | Nobody (generator only) | `repository_dispatch` → pin + regen |

---

## Industry Reference

This pattern matches how Stripe operates:

- Internal canonical OpenAPI is generated and reviewed pre-merge.
- Each API version is snapshotted at release time.
- Artifacts propagate to [stripe/openapi](https://github.com/stripe/openapi), CDN, SDKs, and CLI — consumers never fork the spec manually.

Key lesson: **the spec is an artifact, not a document teams copy around.**

---

## Versioning Strategy

Version **each `*-specification` repository independently** (natural isolation).
Document the scheme in the template’s `CONTRIBUTING` / README. Prefer tags like
`v1.4.2` or `domains-v3@1.4.2` plus an immutable release asset.

Pin keys in consumers may still use `domains/v3` for clarity even though the
source repo is `domains.domain-lifecycle-specification`.

### Two version axes

| Axis | Meaning | Example |
|------|---------|---------|
| API version (folder) | Public contract generation | `v3/` in the Domains specification repo |
| Spec package semver | Revisions of that OpenAPI doc | release `v1.4.2` |

### Artifact format

Each publish produces **that repo’s** OpenAPI tree:

```text
domains-v3-1.2.3/
  openapi.yaml
  openapi.json
  models/…
  manifest.json
  CHANGELOG.md
```

### How consumers consume a release

| Consumer | Mechanism |
|----------|-----------|
| Developer portal | Spec-repo notify → pull artifact → `pins.json` + specs dir → generate → PR |
| CLI / SDKs | Same notify → pin + codegen → PR |
| Service repos | Same pin pattern |

Hardcoded consumer list lives in the **reusable** release/notify workflow shared
by specification repos. See the walkthrough.
---

## Breaking Change Policy

Define explicitly in `api-spec`. Suggested defaults:

| Change | Classification | Required action |
|--------|----------------|-----------------|
| Add optional field | Non-breaking (minor) | Changelog entry |
| Add required field | **Breaking** | Major bump + deprecation period |
| Remove field/endpoint | **Breaking** | `deprecated: true` for 90 days, then major bump |
| Rename field | **Breaking** | Same as remove + add |
| Change field type | **Breaking** | Major bump |
| Change response status code | **Breaking** | Major bump |
| Add enum value | Non-breaking (minor) | Document in changelog |
| Remove enum value | **Breaking** | Major bump |

Enforcement: **oasdiff** (or openapi-diff) in CI compares PR branch against `main`. Any breaking change without `breaking-change` label or major version bump fails the check.

---

## Automated Gate Stack

### Layer 1: Authoring (`*-specification` PR)

| Check | Tool | Blocks merge when |
|-------|------|-------------------|
| Lint | [Spectral](https://stoplight.io/open-source/spectral) | Invalid OpenAPI, missing `operationId`, bad naming |
| Breaking change | [oasdiff](https://github.com/Tufin/oasdiff) | Breaking diff without approval |
| Examples | Spectral custom rule | Missing request/response examples on public ops |
| Ownership | GitHub CODEOWNERS / team | PR merged without capability owner review |

Example Spectral rules to enable early:

- `operation-operationId` — every operation has stable ID (needed for `gddy api call`).
- `info-contact` — contact block present.
- `oas3-api-servers` — servers defined per environment.

### Layer 2: Publish (`*-specification` merge → release + notify)

| Step | Action |
|------|--------|
| 1 | Bundle/dereference OpenAPI (resolve `$ref`, including common-types) |
| 2 | Compute checksums |
| 3 | Create GitHub Release with artifact tarball |
| 4 | **Notify hardcoded consumers** (`repository_dispatch` to docs + CLI) via reusable workflow |

### Layer 3: Downstream sync (docs repo)

| Check | Tool | Blocks merge when |
|-------|------|-------------------|
| Bot PR | GitHub Actions + `peter-evans/create-pull-request` | N/A — opens PR automatically |
| Docs build | Static site generator CI | Broken links, invalid spec embed |
| Human review | Domain owner (optional) | Policy violation |

**Rule:** `developer-ecosystem-documentation` is **read-only for specs** except bot PRs. Direct edits to OpenAPI files in that repo are rejected by branch protection.

### Layer 4: Implementation verification (service repos — phased)

| Check | Tool | Blocks merge when |
|-------|------|-------------------|
| Spec freshness | Diff pinned artifact vs local copy | Local copy edited without bumping pin |
| Runtime contract | [Schemathesis](https://schemathesis.io/) or [Dredd](https://dredd.org/) | HTTP responses ≠ spec |
| Provider verify | [Specmatic](https://docs.specmatic.io/) (optional) | Full contract test failure |

### Layer 5: Consumer verification (CLI, SDKs)

| Check | Tool | Blocks merge when |
|-------|------|-------------------|
| Regen diff | `cargo run -p generate-api-catalog` | Catalog changes without pin bump |
| Codegen diff | progenitor / openapi-generator | Generated client changes unexpectedly |

The `gddy` CLI should pin to **released artifacts** from the relevant
`*-specification` repo (not clone arbitrary tips). Detailed phased checklist:
[API_SPEC_SSOT_IMPLEMENTATION_WALKTHROUGH.md](./API_SPEC_SSOT_IMPLEMENTATION_WALKTHROUGH.md) §8
is the **canonical checklist**. The tables below match that plan.

---

## Roles of `api-spec` vs `*-specification` (do not dual-source)

| Repo | Role | Portal / CLI should pin from it? |
|------|------|----------------------------------|
| `*-specification` (e.g. Domains Lifecycle) | **Editable SSOT** for that modern API | **Yes** — one pin per API |
| `api-spec` | **Architecture review gate** + **legacy** Swagger/OAS catalog (exposure Private→Published; `@API Designers`) | **Only** for APIs whose true editable source is still only there (e.g. older Domains v1/v2). **Never** also copy Domains v3 from `api-spec` |

**Rule:** one public API version → one upstream repo → one pin.

---

## Implementation Work Breakdown

### Phase 0: Baseline & policy (1–2 weeks)

**Deliverable:** Agreed federated SSOT model; inventory of which portal keys map to which `*-specification` (vs legacy `api-spec`).

| ID | Task | Owner | Verification |
|----|------|-------|--------------|
| 0.1 | Inventory portal `registry.ts` keys → upstream repo (spec repo vs `api-spec` vs unknown) | Platform + Docs | Spreadsheet / `pins.json` stub |
| 0.2 | Confirm non-goal: no monorepo migration into `api-spec` | Platform | Written in design docs |
| 0.3 | Confirm hardcoded consumers: docs + `godaddy/cli` (+ optional service) | Platform | List in reusable workflow design |
| 0.4 | Document one-pin-one-source rule (no dual copy) | Platform | In CONTRIBUTING / walkthrough |
| 0.5 | Document versioning + breaking-change policy for specification repos | Platform | Template README / CONTRIBUTING |
| 0.6 | Announce initiative to capability teams | Platform PM | Slack/email |
| 0.7 | Pilot choice: `domains.domain-lifecycle-specification` only | Platform + Domains | Signed off |

---

### Phase 1: Publisher workflow on Domains + reusable template (2–4 weeks)

**Deliverable:** Domains specification repo releases an OpenAPI artifact and notifies docs + CLI.

| ID | Task | Owner | Verification |
|----|------|-------|--------------|
| 1.1 | Spectral + oasdiff on Domains `*-specification` CI | Domains + Platform | Breaking PR fails CI |
| 1.2 | Bundle/dereference OpenAPI (incl. common-types) → artifact + checksum | Platform | Clean bundled OpenAPI |
| 1.3 | GitHub Release on merge/tag from Domains repo | Domains + Platform | Release + asset visible |
| 1.4 | Hardcoded `repository_dispatch` to docs + CLI | Platform | Dispatch received |
| 1.5 | Extract **reusable** release+notify workflow | Platform | Reusable workflow callable |
| 1.6 | Add caller workflow to `api-specification-template` | Platform | New-from-template repos inherit |
| 1.7 | Document rollback (re-notify prior release / pin) | Platform | Runbook once |

**Publish sketch (on each `*-specification`, calling reusable workflow):**

```yaml
# .github/workflows/release.yml  (in domains.domain-lifecycle-specification)
name: Release OpenAPI
on:
  push:
    branches: [main, develop]  # per-repo default

jobs:
  release:
    uses: gdcorp-platform/api-specification-template/.github/workflows/release-and-notify.yml@main
    with:
      package_id: domains/v3
      # consumers hardcoded inside the reusable workflow
    secrets: inherit
```

---

### Phase 2: Developer portal sync (2–4 weeks)

**Deliverable:** Domains release opens a docs PR that updates only `domains-v3` + pin.

| ID | Task | Owner | Verification |
|----|------|-------|--------------|
| 2.1 | Add `openapi-specs/pins.json` per §5.2 schema (`domains/v3` entry) | Docs | Pin committed; schema-valid |
| 2.2 | Add `openapi-specs/package-map.json` (`packageId` → specsDir + registryKey) | Docs | Lookup fails closed for unknown ids |
| 2.3 | `sync-api-spec.yml` on `repository_dispatch` (reads package-map) | Docs | Workflow runs on dispatch |
| 2.4 | Replace specs tree + generate + open PR | Docs | PR title includes version |
| 2.5 | Branch protection: bot-only on `openapi-specs/specs/**` | Docs | Human direct edit blocked |
| 2.6 | Auto-merge patch/minor; human review major | Docs | Policy live |
| 2.7 | Optional: show package version on reference pages | Docs | Footer/badge |
| 2.8 | E2E: Domains tag → portal PR → site shows change | Domains + Docs | Manual test pass |

**Docs sync sketch:**

```yaml
# Triggered by repository_dispatch from the specification repo
on:
  repository_dispatch:
    types: [api-spec-release]
# Pull client_payload.assetUrl, verify sha256, update only mapped path + pins.json
```

---

### Phase 3: CLI consumer pinning (1–3 weeks, parallel with Phase 2)

**Deliverable:** `godaddy/cli` pins Domains release; regen-check enforced.

| ID | Task | Owner | Verification |
|----|------|-------|--------------|
| 3.1 | Add `pins.json` (or equivalent) for `domains/v3` | CLI | Pin committed |
| 3.2 | Sync workflow on same `api-spec-release` dispatch | CLI | PR opened on Domains release |
| 3.3 | Fetch release asset (not live clone tip) for codegen/catalog | CLI | Generator uses pin |
| 3.4 | CI regen-check fails without pin bump | CLI | Drift PR fails |
| 3.5 | Document upgrade runbook | CLI + Platform | Team can follow |

---

### Phase 4: Roll out more APIs + drift (ongoing)

**Deliverable:** Additional `*-specification` repos notify; drift visible.

| ID | Task | Owner | Verification |
|----|------|-------|--------------|
| 4.1 | **Backport** thin release+notify caller to existing `*-specification` repos (portal-backed first; template does not auto-update old repos) | Capability teams + Platform | Each fires dispatch |
| 4.1a | **Script** to open batch PRs (`targets.csv` / package-map → add `.github/workflows/release.yml` caller); App/PAT with cross-repo PR rights | Platform | Dry-run on 2–3 repos; then wave |
| 4.2 | Extend docs/CLI mappings per new `packageId` | Docs + CLI | Pins update independently |
| 4.3 | Weekly job: pins vs latest GitHub Releases on `sourceRepo` | Platform | Slack drift report |
| 4.4 | New template repos include caller by default; aggregator submodule add still manual | Platform | Checklist for new APIs |
| 4.5 | Legacy APIs still only in `api-spec`: decide migrate-to-spec-repo vs stay pinned from `api-spec` | Platform | Per-API decision recorded |

---

### Phase 5: Service implementation verification (later / parallel)

**Deliverable:** Pilot services pin the **same** Domains artifact and contract-test.

| ID | Task | Owner | Verification |
|----|------|-------|--------------|
| 5.1 | Reusable action/script: download pinned release from `sourceRepo` | Platform | Action works |
| 5.2 | Pilot Domains service: pin + Schemathesis (or equivalent) | Domains | CI fails on impl drift |
| 5.3 | Block local OpenAPI edits without pin bump | Platform | CI gate |
| 5.4 | Roll out to other GA services on a schedule | Domain owners | Adoption tracker |

```yaml
- name: Download pinned specification release
  run: |
    # Read pins.json / .api-spec-pin → sourceRepo + tag
    gh release download "$TAG" --repo "$SOURCE_REPO" -D spec/
- name: Contract test
  run: schemathesis run spec/openapi.yaml --base-url "$TEST_SERVICE_URL" --checks all
```

---

## Roles & Responsibilities

| Role | Responsibilities |
|------|------------------|
| **Capability team** | Own `*-specification` OpenAPI; version bumps; merge releases |
| **Platform team** | Reusable release+notify workflow, template, drift jobs, policy |
| **Docs team** | Sync workflow, `pins.json`, merge pin PRs; no hand-edited OpenAPI |
| **CLI team** | Sync workflow, pins, regen-check |
| **Service team** | Pin artifact; keep runtime matching; no forked SSOT |
| **API Designers (`api-spec`)** | Continue architecture review for APIs still on that process / legacy catalog |

---

## Migration Strategy (rollout waves — not monorepo)

Do **not** consolidate into `api-spec`. Roll out **automation** wave by wave:

1. **Pilot:** Domains Lifecycle v3 (`domains.domain-lifecycle-specification`) → docs + CLI.
2. **Wave 2:** High-traffic commerce/capability `*-specification` repos already on the portal.
3. **Wave 3:** Remaining Published APIs on `*-specification`.
4. **Legacy:** APIs that exist only in `api-spec` — either migrate into a new `*-specification` (from template) or keep a separate pin from `api-spec` until retired. Never dual-source.

For each API:

1. Freeze manual portal edits for that key.
2. Enable release+notify on its specification repo.
3. Add mapping + pin on docs/CLI.
4. Remove stale hand copies after grace period.

---

## Success Metrics

| Metric | Target (6 months) |
|--------|-------------------|
| Modern portal APIs with pin → `*-specification` release | 100% of in-scope GA |
| Docs manual OpenAPI edits for pinned APIs | 0 |
| Mean time Domains release → docs PR | &lt; 1 hour |
| CLI pin matches Domains release when intended | Measurable via pins |
| Incidents from spec/docs drift | Trending to zero |

---

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| Copy-paste workflows across 80 repos | Reusable workflow in template |
| Missed dispatch | Weekly drift job |
| Dual-sourcing same API from `api-spec` + spec repo | Inventory + one-pin rule; CI on pins |
| Cross-org token for notify | GitHub App early |
| Bot PR backlog | Auto-merge patch/minor |
| Teams still hand-edit portal YAML | Branch protection |

---

## Open Questions

1. **Tag scheme** per repo: `v1.4.2` vs `domains-v3@1.4.2`?
2. **Semver vs date-version** for package releases?
3. **GraphQL** — keep `SOURCE.md` refresh or same notify pattern?
4. **Which legacy `api-spec` APIs** must stay on portal long-term?
5. Does Published review still require an `api-spec` PR **in addition to** the `*-specification` repo, or is review shifting?

---

## Appendix A: Tooling Reference

| Tool | Purpose | License |
|------|---------|---------|
| [Spectral](https://stoplight.io/open-source/spectral) | OpenAPI lint | MIT |
| [oasdiff](https://github.com/Tufin/oasdiff) | Breaking change detection | Apache 2.0 |
| [Schemathesis](https://schemathesis.io/) | Property-based contract testing | MIT |
| [Dredd](https://dredd.org/) | Request/response validation | MIT |
| [Specmatic](https://docs.specmatic.io/) | Contract-driven testing + stubs | Commercial/OSS |
| [openapi-generator](https://openapi-generator.tech/) | Client/server codegen | Apache 2.0 |
| [Redocly CLI](https://redocly.com/docs/cli/) | Bundle, lint, preview docs | Commercial |

---

## Appendix B: Related Internal Systems

| System | Relationship to SSOT |
|--------|---------------------|
| `gdcorp-platform/*-specification` | **Canonical editable OpenAPI** per capability |
| `gdcorp-platform/api-specification-template` | Bootstrap + reusable release/notify |
| `gdcorp-platform/api-spec` | Architecture review + legacy catalog — not Domains v3 SSOT |
| `gdcorp-platform/api-specification-aggregator` | Discovery (submodules) |
| `developer-ecosystem-documentation` | Downstream pin + generate |
| `godaddy/cli` | Downstream pin + codegen/catalog |
| [Domains v3 portal](https://developer.godaddy.com/en/docs/references/rest/domains/v3) | Published derivative of Domains pin |

---

## Appendix C: Phase Summary Timeline

```
Phase 0 ──► Phase 1 ──► Phase 2 ──► Phase 3
(policy)    (Domains     (docs       (CLI pin)
             release+     sync)
             notify)
 1-2 wk      2-4 wk       2-4 wk      1-3 wk
                │              │
                └──── Phase 4 (roll out more APIs + drift)
                └──── Phase 5 (service contract tests, later)
```

**Pilot (Domains → docs + CLI):** ~6–10 weeks. Broader adoption: rolling.

---

## References

- [Walkthrough (implementation)](./API_SPEC_SSOT_IMPLEMENTATION_WALKTHROUGH.md)
- [Repo analysis](./API_SPEC_REPO_ANALYSIS_SSOT.md)
- [Stripe: API versioning](https://stripe.com/blog/api-versioning)
- [Stripe: How API changes flow into developer products](https://stripe.dev/blog/how-api-changes-flow-into-stripes-developer-products)
- [stripe/openapi](https://github.com/stripe/openapi)
- [GoDaddy REST API Reference](https://developer.godaddy.com/en/docs/references/rest)
- [api-spec README (review process)](https://github.com/gdcorp-platform/api-spec/blob/master/README.md)
- [domains.domain-lifecycle-specification](https://github.com/gdcorp-platform/domains.domain-lifecycle-specification)
- [api-specification-template](https://github.com/gdcorp-platform/api-specification-template)
