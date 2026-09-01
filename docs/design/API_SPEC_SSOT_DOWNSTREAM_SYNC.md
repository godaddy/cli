# Design: API Spec Single Source of Truth + Downstream Sync

---

## Problem Statement

GoDaddy's developer platform API specifications currently flow through multiple repositories and organizations.

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

- Establish **one editable source** for each API contract (`gdcorp-platform/api-spec`).
- **Automate propagation** to developer docs and consumer repos — no hand-editing downstream copies.
- **Block breaking changes** at PR time unless explicitly versioned and approved.
- **Prove implementation matches spec** before service releases (phased rollout).
- Work across **org boundaries** via versioned artifacts (GitHub Releases, npm package, or internal registry).

## Non-Goals

- Replacing Pact/consumer-driven contracts for all APIs in phase 1 (optional later layer).
- Mandating Backstage/catalog setup before SSOT is working.
- Consolidating all team implementation repos into one org.

---

## Recommended Architecture

### Principle: Write once, publish many — **per product/version tree**

Canonical edits happen in **`api-spec/apis/<product>/<version>/`** (e.g.
`apis/domains/v3/`). Each version tree has its **own package semver / release
artifact**. A Domains v3 change publishes `domains/v3@1.4.2` only — it does
**not** bump Domains v2, Shoppers, Hosting, or a repo-wide `api-spec` version
that consumers must all take.

Everything outside `api-spec` is a **pinned derivative** of those packages.

```
┌─────────────────────────────────────────────────────────────────┐
│  gdcorp-platform/api-spec                                       │
│  apis/domains/{v2,v3}/   apis/shoppers/v1/   apis/certificates/v1/ … │
│  • Humans edit OpenAPI in their product/version folder          │
│  • Spectral + oasdiff path-filtered per version tree            │
│  • CODEOWNERS per apis/<product>/                               │
│  • Architecture review process stays in this repo               │
└──────────────────────────┬──────────────────────────────────────┘
                           │ merge → release ONLY changed trees
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│  Publish pipeline (per apis/<product>/<version>/)               │
│  • Validate + bundle that tree                                  │
│  • Version: domains/v3@1.4.2 (independent of domains/v2, etc.)  │
│  • Upload immutable artifact (checksum + CHANGELOG)             │
└──────────────────────────┬──────────────────────────────────────┘
                           │
           ┌───────────────┼───────────────┐
           ▼               ▼               ▼
   ┌──────────────┐ ┌──────────────┐ ┌──────────────────────┐
   │ docs portal  │ │ service repos│ │ consumer repos       │
   │ pin + sync   │ │ (pin+verify) │ │ (gddy, SDKs, etc.)   │
   │ that package │ │              │ │ pin per product/ver  │
   └──────────────┘ └──────────────┘ └──────────────────────┘
```

> **Migration:** Today many contracts live in
> `gdcorp-platform/<domain>.<capability>-specification`. Treat those as sources
> to import into `api-spec/apis/<product>/<version>/`; do not keep spawning new
> repos long-term. Details:
> [API_SPEC_REPO_ANALYSIS_SSOT.md](./API_SPEC_REPO_ANALYSIS_SSOT.md) Part 5b.

### Repository roles

| Repository | Role | Who edits | Sync mechanism |
|------------|------|-----------|----------------|
| `gdcorp-platform/api-spec` | **Canonical SSOT monorepo** (`apis/<product>/<version>/`) | Capability teams via PR + CODEOWNERS | N/A — source; publishes per-tree artifacts |
| `*-specification` (legacy) | Migration source until cutover | Same teams | Archive after import |
| `api-specification-aggregator` | Temporary discovery while federated | Submodule pins | Retire when monorepo is complete |
| `gdcorp-commerce/developer-ecosystem-documentation` | **Published developer docs** | Guides: humans; `openapi-specs/specs/`: bot only | Automated PR from **per-tree** releases → `registry.ts` pipeline |
| Team service repos | **Implementation** | Service teams | Pin that product/version package; CI verifies |
| `godaddy/cli` | **CLI API catalog / clients** | Nobody (generator only) | Pin per product/version; regen check |

---

## Industry Reference

This pattern matches how Stripe operates:

- Internal canonical OpenAPI is generated and reviewed pre-merge.
- Each API version is snapshotted at release time.
- Artifacts propagate to [stripe/openapi](https://github.com/stripe/openapi), CDN, SDKs, and CLI — consumers never fork the spec manually.

Key lesson: **the spec is an artifact, not a document teams copy around.**

---

## Versioning Strategy

Version **each `apis/<product>/<version>/` tree independently**. Document the
scheme in `api-spec/CONTRIBUTING.md`. Do **not** ship a single repo-wide semver
that republishes every API.

Prefer nested paths (`apis/domains/v3/`) over flat folder names (`domains-v3/`).
Flat ids remain OK only as **pin/registry keys**.

### Two version axes

| Axis | Meaning | Example |
|------|---------|---------|
| API version (folder) | Public contract generation | `v3` under `apis/domains/` |
| Spec package semver | Revisions of that OpenAPI doc | `domains/v3@1.4.2` |

### Option A: Semver per version tree (recommended for REST OpenAPI)

- Package id: `domains/v3`, `shoppers/v1`, …
- Version: `1.2.3` — patch = docs/examples only; minor = additive; major = breaking **within that API generation**.
- Tag: `domains/v3@1.2.3`.
- New public HTTP generation → new folder (`domains/v4/`), not only a package bump under `v3`.

### Option B: Date-version per version tree (Stripe-style)

- `domains/v3@2026-08-28` — rolling versions named by release date for that tree.
- Backward-incompatible changes ship in new date versions; old versions supported for N months.

### Artifact format

Each publish produces **one version tree** (not the whole monorepo):

```text
domains-v3-1.2.3/
  openapi.yaml
  openapi.json
  models/…
  manifest.json     # product, apiVersion, packageVersion, checksums
  CHANGELOG.md      # domains/v3 only
```

Optional generated index (not a consumer pin):

```text
catalog.json        # { "domains/v3": "1.2.3", "shoppers/v1": "2.1.0", … }
```

`manifest.json` example:

```json
{
  "product": "domains",
  "apiVersion": "v3",
  "packageVersion": "1.2.3",
  "publishedAt": "2026-08-28T14:00:00Z",
  "files": [
    { "path": "openapi.yaml", "sha256": "abc123..." }
  ]
}
```

### How consumers consume a release

| Consumer | Mechanism |
|----------|-----------|
| Developer portal | Release webhook → bot updates only that key under `openapi-specs/specs/` + `pins.json` → `bundle-specs` / `generate` → docs PR |
| CLI / SDKs | Pin file lists `domains/v3@1.2.3`; fetch artifact; regen that client only |
| Service repos | Same pin; contract tests against the artifact |

See analysis Part 5b for the full pin shape and path-filtered publish flow.
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

### Layer 1: Authoring (api-spec PR)

| Check | Tool | Blocks merge when |
|-------|------|-------------------|
| Lint | [Spectral](https://stoplight.io/open-source/spectral) | Invalid OpenAPI, missing `operationId`, bad naming |
| Breaking change | [oasdiff](https://github.com/Tufin/oasdiff) | Breaking diff without approval |
| Examples | Spectral custom rule | Missing request/response examples on public ops |
| Ownership | GitHub CODEOWNERS | PR merged without domain owner review |

Example Spectral rules to enable early:

- `operation-operationId` — every operation has stable ID (needed for `gddy api call`).
- `info-contact` — contact block present.
- `oas3-api-servers` — servers defined per environment.

### Layer 2: Publish (api-spec merge to main)

| Step | Action |
|------|--------|
| 1 | Bundle/dereference specs (resolve `$ref`) |
| 2 | Compute checksums |
| 3 | Create GitHub Release `v1.2.3` with artifact tarball |
| 4 | Trigger downstream sync workflows |

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
| Regen diff | `cargo run -p generate-api-catalog` | Catalog changes without manifest pin bump |
| Codegen diff | progenitor / openapi-generator | Generated client changes unexpectedly |

The `gddy` CLI already discovers specs from `gdcorp-platform` repos via `generate-api-catalog`. After SSOT adoption, it should pin to **released artifacts** from `api-spec` rather than cloning arbitrary repos.

---

## Implementation Work Breakdown

### Phase 0: Baseline & Policy (2–4 weeks)

**Deliverable:** api-spec has lint + breaking-change gates; policy documented.

| ID | Task | Owner | Verification |
|----|------|-------|--------------|
| 0.1 | Inventory all spec locations (api-spec, docs repo, team repos, gddy `schemas/api/`) | Platform | Spreadsheet or catalog JSON |
| 0.2 | Assign CODEOWNERS per API domain in api-spec | Platform + domain leads | PR requires owner approval |
| 0.3 | Add Spectral config (`.spectral.yaml`) with baseline rules | Platform | `spectral lint` passes locally |
| 0.4 | Add oasdiff breaking-change check to api-spec PR CI | Platform | PR with removed field fails CI |
| 0.5 | Write `CONTRIBUTING.md`: versioning, breaking policy, deprecation | Platform | Reviewed by 2+ domain owners |
| 0.6 | Add `deprecated: true` + removal date convention to policy | Platform | Example in CONTRIBUTING |
| 0.7 | Announce SSOT initiative to API-owning teams | Platform PM | Slack/email with timeline |

**Manual test:**

```bash
# In api-spec repo
spectral lint openapi/domains-v3.yaml
oasdiff breaking openapi/domains-v3.yaml openapi/domains-v3-main.yaml
```

---

### Phase 1: Publish Pipeline + Docs Sync (4–6 weeks)

**Deliverable:** Merging api-spec main publishes versioned artifact and opens PR to docs repo.

| ID | Task | Owner | Verification |
|----|------|-------|--------------|
| 1.1 | Create `publish.yml` workflow on api-spec main merge | Platform | Release artifact on test merge |
| 1.2 | Implement spec bundling/dereference step | Platform | No unresolved `$ref` in artifact |
| 1.3 | Generate `manifest.json` with checksums | Platform | Manifest validates against files |
| 1.4 | Create GitHub Release with semver tag | Platform | Release visible with assets |
| 1.5 | Build docs-sync bot workflow | Platform | Bot opens PR to docs repo |
| 1.6 | Configure docs repo branch protection (no direct spec edits) | Docs team | Direct push rejected |
| 1.7 | Add spec version badge to docs site (version + date) | Docs team | Badge shows current release |
| 1.8 | Migrate one pilot API (e.g. Domains v3) end-to-end | Domains team | Docs match api-spec release |
| 1.9 | Document rollback procedure (re-publish previous version) | Platform | Runbook tested once |

**Publish workflow sketch (api-spec):**

```yaml
# .github/workflows/publish.yml
name: Publish API Spec
on:
  push:
    branches: [main]

jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Lint
        run: npx @stoplight/spectral-cli lint '**/*.{yaml,yml,json}'
      - name: Bundle specs
        run: ./scripts/bundle-specs.sh
      - name: Create release
        uses: softprops/action-gh-release@v2
        with:
          tag_name: v${{ steps.version.outputs.version }}
          files: dist/**
```

**Docs sync workflow sketch:**

```yaml
# Triggered by repository_dispatch from api-spec publish
- name: Update specs in docs repo
  run: |
    curl -L -o dist/api-spec-${{ inputs.version }}.tar.gz \
      https://github.com/gdcorp-platform/api-spec/releases/download/v${{ inputs.version }}/api-spec.tar.gz
    tar xzf dist/api-spec-*.tar.gz -C content/specs/
- name: Open PR
  uses: peter-evans/create-pull-request@v6
  with:
    title: "chore(specs): sync api-spec v${{ inputs.version }}"
    branch: sync/api-spec-v${{ inputs.version }}
```

**Manual test:**

1. Merge a non-breaking change to api-spec main.
2. Confirm GitHub Release created.
3. Confirm bot PR opened in developer-ecosystem-documentation.
4. Merge bot PR; verify docs site shows updated spec.

---

### Phase 2: Service Repo Adoption (6–10 weeks)

**Deliverable:** Pilot services pin published artifact and verify runtime in CI.

| ID | Task | Owner | Verification |
|----|------|-------|--------------|
| 2.1 | Create reusable GitHub Action: `pin-api-spec@v1` | Platform | Action downloads + pins artifact |
| 2.2 | Add `api-spec-version` field to service repo config | Platform | Documented in template |
| 2.3 | Pilot Domains service: pin + Schemathesis in CI | Domains team | CI fails on impl drift |
| 2.4 | Pilot Hosting service: same pattern | Hosting team | CI green on matching impl |
| 2.5 | Add PR check: local spec must match pinned artifact | Platform | Edited local copy fails CI |
| 2.6 | Template repo for new services includes pin + verify | Platform | New service scaffold works |
| 2.7 | Roll out to remaining GA APIs (rolling schedule) | Domain owners | Tracker shows adoption % |
| 2.8 | Add deprecation enforcement: code cannot remove before spec marks deprecated | Platform | CI catches premature removal |

**Service repo CI sketch:**

```yaml
- name: Download pinned api-spec
  run: |
    VERSION=$(cat .api-spec-version)
    gh release download "v${VERSION}" --repo gdcorp-platform/api-spec -D spec/

- name: Contract test
  run: |
    schemathesis run spec/domains-v3/openapi.yaml \
      --base-url "${{ env.TEST_SERVICE_URL }}" \
      --checks all
```

**Manual test:**

1. Deploy service to test environment.
2. Run Schemathesis locally against test URL.
3. Introduce intentional response shape change; confirm CI fails.

---

### Phase 3: Consumer Pinning (4–6 weeks, parallel with Phase 2)

**Deliverable:** gddy CLI and other consumers pin to released spec versions.

| ID | Task | Owner | Verification |
|----|------|-------|--------------|
| 3.1 | Add `api-spec-version` pin to cli `generate-api-catalog` manifest | CLI team | Manifest file committed |
| 3.2 | Change catalog generator to fetch from GitHub Release, not live repo clone | CLI team | Generator uses release asset |
| 3.3 | CI check: regen produces no diff unless pin bumped | CLI team | PR with drift fails CI |
| 3.4 | Document consumer upgrade process in api-spec CHANGELOG | Platform | CLI team can follow runbook |
| 3.5 | Identify other consumers (SDK gen, portal, partner tools) | Platform | Consumer inventory complete |
| 3.6 | Roll out pinning to top 3 consumers | Consumer owners | Each has CI gate |

**Manual test:**

```bash
cd rust
# Bump pin in schemas/api-source-manifest.toml (or equivalent)
cargo run -p generate-api-catalog
git diff schemas/api/   # should be empty if pin unchanged
```

---

### Phase 4: Observability & Governance (ongoing)

**Deliverable:** Drift is visible; owners accountable.

| ID | Task | Owner | Verification |
|----|------|-------|--------------|
| 4.1 | Weekly drift job: compare api-spec vs all registered service copies | Platform | Report in Slack/email |
| 4.2 | Dashboard: spec version per service vs latest release | Platform | Stale services visible |
| 4.3 | Add `catalog-info.yaml` per API (Backstage-compatible) | Domain owners | Entity links to spec URL |
| 4.4 | Quarterly review: breaking changes, deprecation cleanup | Platform + owners | Meeting notes archived |
| 4.5 | Optional: Pact/Specmatic for top 5 provider-consumer pairs | Platform | Compatibility matrix green |

---

## Roles & Responsibilities

| Role | Responsibilities |
|------|------------------|
| **Platform team** | Own api-spec repo, publish pipeline, policies, shared Actions |
| **API domain owner** | Review PRs for their domain; approve breaking changes |
| **Service team** | Pin artifact; keep implementation matching spec; no local spec edits |
| **Docs team** | Merge bot PRs; maintain docs site; no manual spec authoring |
| **Consumer team (CLI/SDK)** | Pin releases; regen on bump; report spec gaps via api-spec issues |

---

## Migration Strategy

Do not big-bang migrate all APIs at once.

1. **Pilot:** Domains v3 (already used by gddy CLI) — highest visibility, existing catalog generator.
2. **Wave 2:** Hosting, Shoppers, Certificates — REST APIs with active CLI/docs usage.
3. **Wave 3:** Remaining public APIs.
4. **Internal-only APIs:** Optional separate artifact channel or `preview/` directory (Stripe pattern).

For each wave:

1. Freeze manual edits in docs repo for that API.
2. Enable publish + bot sync.
3. Notify service team to pin + add contract tests.
4. Remove stale copies from team repos after 30-day grace period.

---

## Success Metrics

| Metric | Target (6 months) |
|--------|-------------------|
| APIs published via SSOT pipeline | 100% of public GA APIs |
| Docs repo manual spec edits | 0 |
| Service repos with pinned artifact + CI verify | 80% of GA services |
| Consumer repos with pinned artifact | 100% of platform-owned consumers |
| Incidents caused by spec/impl drift | Trending to zero |
| Mean time to propagate spec change to docs | < 24 hours (automated) |

---

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| Teams resist giving up local spec copies | Phased rollout; template repos; CI makes local edits fail |
| Bot PR backlog in docs repo | Auto-merge for patch/minor; owner review only for major |
| Cross-org GitHub App permissions | Platform admin sets up org-wide app early |
| Flaky contract tests | Run against dedicated test env; retry transient failures only |
| api-spec becomes bottleneck | CODEOWNERS per domain; domain owners approve, platform merges |

---

## Open Questions

1. **Who owns api-spec today vs who should?** Platform vs federated domain owners?
2. **Semver vs date-version?** REST public APIs often use semver; Stripe uses dates.
3. **GraphQL APIs?** May need schema registry instead of OpenAPI-only pipeline.
4. **Internal vs external specs?** Separate artifact channels (`/latest/` vs `/preview/` like stripe/openapi)?
5. **OTE vs prod spec divergence?** Environment-specific server URLs vs separate spec files?

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
| `gdcorp-platform/api-spec` | Becomes canonical source |
| `developer-ecosystem-documentation` | Downstream sync target (bot PRs) |
| `godaddy/cli` `generate-api-catalog` | Consumer — pin to releases |
| `godaddy/cli` `domains-client` | Generated from pinned Domains v3 spec |
| `gddy api call` | Depends on stable `operationId` in spec |

---

## Appendix C: Phase Summary Timeline

```
Phase 0 ──► Phase 1 ──► Phase 2 ──► Phase 3
(baseline)   (publish)   (services)  (consumers)
 2-4 wk       4-6 wk      6-10 wk     4-6 wk
                │              │
                └──── Phase 4 (ongoing: drift, catalog, governance)
```

**Total to full adoption:** ~6–9 months with phased waves.

---

## References

- [Stripe: API versioning](https://stripe.com/blog/api-versioning)
- [Stripe: How API changes flow into developer products](https://stripe.dev/blog/how-api-changes-flow-into-stripes-developer-products)
- [stripe/openapi](https://github.com/stripe/openapi)
- [Specmatic: Central contract repository](https://docs.specmatic.io/contract_driven_development/contract_repositories/central_contract_repository)
- [DeployIt: CI-first OpenAPI workflow](https://deployit.ai/blog/keep-api-docs-in-sync-with-code-a-ci-first-workflow)
- [GoDaddy REST API Reference](https://developer.godaddy.com/en/docs/references/rest)
