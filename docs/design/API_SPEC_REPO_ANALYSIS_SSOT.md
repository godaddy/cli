# Analysis: API Spec Repos Today vs SSOT + Downstream Sync

This document analyzes the **actual** repositories involved in GoDaddy's API
specification and developer documentation flow, then maps the
[SSOT + Downstream Sync](./API_SPEC_SSOT_DOWNSTREAM_SYNC.md) design onto that
reality.

**Repos analyzed (August 2026):**

| Repository | Org | Role (today) |
|------------|-----|--------------|
| [`gdcorp-platform/api-spec`](https://github.com/gdcorp-platform/api-spec) | Platform | Architecture review registry + legacy Swagger/OpenAPI store |
| [`gdcorp-platform/*-specification`](https://github.com/gdcorp-platform) | Platform | Per-capability **modern** OpenAPI SSOT repos |
| [`gdcorp-platform/api-specification-aggregator`](https://github.com/gdcorp-platform/api-specification-aggregator) | Platform | Submodule wrapper over all `*-specification` repos + `api-spec` |
| [`gdcorp-commerce/developer-ecosystem-documentation`](https://github.com/gdcorp-commerce/developer-ecosystem-documentation) | Commerce | Developer portal — vendored specs + generated REST/GraphQL reference |

---

## Executive summary

### What exists today

1. **Modern public APIs** (Domains Lifecycle v3, Commerce Businesses, Channels,
   Stores, Webhooks, etc.) currently live in dedicated
   `gdcorp-platform/<domain>.<capability>-specification` repositories — **not**
   primarily in `api-spec`.
2. **`api-spec`** remains the long-standing **architecture review gate** and
   home for older Swagger 1.2 / 2.0 / early OpenAPI 3 specs (including historical
   Domains v1/v2).
3. **`developer-ecosystem-documentation`** already treats OpenAPI under
   `openapi-specs/specs/` as the portal’s generation input, but those files are
   **vendored copies**. Sync from upstream is mostly **manual** (except GraphQL
   schemas that document `SOURCE.md` + refresh commands).
4. **Live drift already exists.** Example: Domains Lifecycle v3 tip on
   `domains.domain-lifecycle-specification` (2026-08-28) is ahead of the last
   domains-v3 sync commit in the docs repo (2026-08-14).

### Recommended target (preferred)

**Keep the existing federated `*-specification` repos as SSOT.** Do not migrate
into an `api-spec` monorepo for this design.

Each capability repo (from
[`api-specification-template`](https://github.com/gdcorp-platform/api-specification-template)):

1. Authors OpenAPI under `v{N}/schemas/`.
2. Publishes its **own** GitHub Release + OpenAPI artifact.
3. Runs a **release + notify** workflow (shared reusable workflow) that
   `repository_dispatch`es a **hardcoded** list of consumers (&lt;3): developer
   portal, `godaddy/cli`, optional service.
4. Consumers pull that artifact, update **only that** pin/path, open a PR.

```
domains.domain-lifecycle-specification   ◄── edit + release domains/v3
commerce.businesses-specification        ◄── edit + release businesses/…
apis.webhooks-specification              …
        │
        │ each repo: Release + notify (reusable workflow)
        ▼
developer-ecosystem-documentation  (pins.json + specs/<key>/)
godaddy/cli                        (pins + codegen)
```

`api-spec` remains review/legacy. Aggregator remains discovery (manual submodule
add). Isolation of releases is inherent (separate repos) — a Domains change
never republishes Shoppers.

See the [implementation walkthrough](./API_SPEC_SSOT_IMPLEMENTATION_WALKTHROUGH.md)
and Part 5 below.

---

## Part 1 — `gdcorp-platform/api-spec`

### What it is

Described as: **“Definitive Swagger Specifications for Reviewed APIs.”**

It is primarily an **architecture/API-design review repository**:

- Teams submit Swagger/OpenAPI PRs for review based on exposure level
  (Private / Protected / Public / Published).
- Automated validation must pass (`npm test` / `check-2.0` / `check-3.0`).
- Published APIs require Architecture Review for all changes; breaking changes
  need Integrator Review.
- Hosts a Global API Registry / SwaggerUI / Redoc-style browsing experience.

### How specs are organized

| Path | Format | Purpose |
|------|--------|---------|
| `spec/`, `spec1/` | Swagger 1.2 (+ converted YAML) | Legacy public/internal APIs |
| `spec2/external/`, `spec2/internal/` | Swagger 2.0 | Older reviewed APIs |
| `spec3/external/`, `spec3/internal/` | OpenAPI 3.0 | Newer reviewed APIs |
| `spec3/common/`, `spec2/common/` | Shared models | Reusable errors, parameters, properties |
| `search-urls.js` | URL list | Extra live specs included in registry search |
| `knownBroken.json` | Exemptions | Specs allowed to fail certain compliance checks |

**Example — Domains in `api-spec`:** historical Domains paths under `spec/v1/`,
`spec/v2/`, `spec2/external/domainsapi/`, diagrams, etc. These are **not** the
Domains Lifecycle Management API v3 (`openapi: 3.1.0`) used by the developer
portal and `gddy` CLI.

### Automation already present

| Mechanism | Location | What it does |
|-----------|----------|--------------|
| PR CI | `.github/workflows/api-spec-check.yaml` | Runs `npm run check-2.0 && npm run check-3.0` on PRs to `master` |
| Lint/validate | `package.json` scripts | ESLint, swagger-tools, oas-validator, mocha compliance tests |
| URL index generation | `generate-urls-*.js` | Updates `swagger-ui-urls.js` when specs are added |
| Preview | `npm run preview` | Local Redoc-style registry |

### Gaps relative to SSOT + downstream sync

| Gap | Detail |
|-----|--------|
| Not the Domains v3 SSOT | Domains Lifecycle v3 lives in `domains.domain-lifecycle-specification` |
| No publish artifact pipeline | Merge does not produce a versioned release package for consumers |
| No docs-repo sync | Nothing automatically updates `developer-ecosystem-documentation` |
| Mixed eras | Swagger 1.2 through OpenAPI 3 coexist; versioning policy is process-based, not release-based |
| Review ≠ distribution | Passing architecture review does not propagate to the public developer portal |

### Takeaway (as-is vs preferred)

**Today and preferred:** Treat each `*-specification` repo as the editable SSOT
for that capability. Add release + notify workflows (via template/reusable
workflow) so docs and CLI pin published artifacts.

**`api-spec`:** architecture review gate + legacy catalog — not the Domains v3
write path, and **not** the consolidation target for this design.

---

## Part 2 — Modern SSOT: `*-specification` repos

> **Status:** this is both the **current** layout and the **preferred** SSOT.
> New repos are created from
> [`api-specification-template`](https://github.com/gdcorp-platform/api-specification-template)
> (`is_template: true`). GitHub API shows e.g. Domains Lifecycle,
> `commerce.businesses-specification`, and `apis.webhooks-specification` all
> list that template as `template_repository`.

### Pattern

Platform teams own dedicated repos named:

```text
gdcorp-platform/<domain>.<capability>-specification
```

Examples:

| Repo | Capability |
|------|------------|
| [`domains.domain-lifecycle-specification`](https://github.com/gdcorp-platform/domains.domain-lifecycle-specification) | Domains discovery, quote/register, DNS, renewals, transfers |
| [`commerce.businesses-specification`](https://github.com/gdcorp-platform/commerce.businesses-specification) | Commerce businesses |
| [`commerce.channels-specification`](https://github.com/gdcorp-platform/commerce.channels-specification) | Channels |
| [`commerce.stores-specification`](https://github.com/gdcorp-platform/commerce.stores-specification) | Stores |
| [`apis.webhooks-specification`](https://github.com/gdcorp-platform/apis.webhooks-specification) | Webhook management |
| [`common-types-specification`](https://github.com/gdcorp-platform/common-types-specification) | Shared types |
| … | Many more commerce/payments/risk/hosting specs |

There is also [`api-specification-template`](https://github.com/gdcorp-platform/api-specification-template)
for bootstrapping new ones.

### Domains Lifecycle v3 (worked example)

**Upstream structure:**

```text
domains.domain-lifecycle-specification/
  README.md
  v3/
    docs/          # RELEASE-NOTES.md, USER-GUIDE.md
    samples/
    schemas/
      openapi.yaml # OpenAPI 3.1.0 — Domain Lifecycle Management API
      models/
      enums/
      common-types/  # often submodule or vendored shared types
```

**Portal copy:**

```text
developer-ecosystem-documentation/
  openapi-specs/specs/domains-v3/v3/schemas/openapi.yaml
  openapi-specs/specs/domains-v3/v3/schemas/models/...
```

Same relative layout — strong evidence the docs tree is a **vendored snapshot**
of the specification repo, not an independently authored contract.

**Drift evidence (sampled Aug 2026):**

| Location | Latest relevant activity |
|----------|--------------------------|
| `domains.domain-lifecycle-specification@develop` | 2026-08-28 — “Add GET registration schema” |
| Docs `openapi-specs/specs/domains-v3` | 2026-08-14 — DNS PUT + Fee/premium consent sync |

~2 weeks of upstream changes may be missing from the public portal until someone
manually copies them and runs `npm run generate`.

### Aggregator

[`api-specification-aggregator`](https://github.com/gdcorp-platform/api-specification-aggregator)
lists all `*-specification` repos (and `api-spec`) as **git submodules** so
engineers can search the whole surface area in one checkout.

Important:

- Aggregator is a **read/discovery** convenience.
- It is **not** a publish pipeline and does not sync to the docs portal.
- New modules must be added manually to `.gitmodules`.

### Takeaway

The **canonical editable OpenAPI** for a modern Published API is:

```text
gdcorp-platform/<domain>.<capability>-specification
```

Bootstrapped from `api-specification-template`. Missing piece for SSOT + sync:
**release artifact + notify workflow** on each repo (reusable), plus consumer
pin PRs on docs/CLI. Aggregator is discovery only (manual submodule add).

---

## Part 3 — `gdcorp-commerce/developer-ecosystem-documentation`

### What it is

Next.js + Fumadocs developer portal for GoDaddy Platform Applications.

Two documentation layers:

| Layer | Path | Authoring |
|-------|------|-----------|
| Hand-authored guides | `content/docs/api-users/`, getting-started, tutorials | Humans edit MDX |
| Generated API reference | `content/docs/references/rest/`, GraphQL refs | Generated from vendored specs — **do not hand-edit** |

Authoring guide is explicit:

> Generated reference docs are enforced upstream via Spectral linting in source
> repos. If a generated page reads poorly, fix the OpenAPI spec — don't work
> around it here.

### Where specs live in the portal

```text
openapi-specs/
  registry.ts          # SSOT for portal generation wiring
  specs/               # Vendored YAML/JSON OpenAPI (+ GraphQL schemas)
    domains-v3/
    domains-v1.yaml
    domains-v2.yaml
    shoppers.yaml
    external/          # Migrated from developer.godaddy.com legacy
    businesses/
    channels/
    stores/
    catalog-graphql/   # has SOURCE.md → catalog-api-v2
    orders-graphql/    # has SOURCE.md → order-api
    taxes/graphql/     # has SOURCE.md + overlays
    ...
  bundled/             # Output of npm run bundle-specs (JSON artifacts)
```

### How specs are linked to generated docs

**Registry** (`openapi-specs/registry.ts`) is the portal’s wiring SSOT:

```ts
{
  key: 'domains-v3',
  documentKey: './openapi-specs/specs/domains-v3/v3/schemas/openapi.yaml',
  source: './openapi-specs/specs/domains-v3/v3/schemas/openapi.yaml',
  bundled: './openapi-specs/bundled/domains-v3.json',
  output: './content/docs/references/rest/domains/v3',
}
```

Pipeline:

```text
registry.ts
    │
    ▼
npm run bundle-specs          # @scalar/json-magic bundle + portal transforms
    │
    ▼
openapi-specs/bundled/*.json
    │
    ▼
npm run generate              # fumadocs-openapi → MDX under output/
    │
    ▼
content/docs/references/rest/...
    │
    ▼
lib/openapi.ts + <APIPage>    # runtime loads same documentKey
```

Validators:

| Script | Purpose |
|--------|---------|
| `npm run lint:openapi` | Registry consistency — source + bundled + output dirs exist |
| `npm run generate:check` | Regen GraphQL refs then `git diff --exit-code` |
| `npm run lint:all` | Full docs lint suite including OpenAPI |

### Portal-specific transforms (adapter layer)

`scripts/bundle-specs.ts` applies **docs-only** transforms before bundling.
Example for Domains v3:

- `oauth2 → bearerAuth` (portal auth UX)
- Strip `X-Shopper-Id` parameter refs

These transforms are **legitimate portal concerns**. They should remain in the
docs repo as an adapter — **not** be written back into the specification SSOT.

### Provenance documentation (uneven today)

| Spec family | Provenance documented? | Upstream |
|-------------|------------------------|----------|
| `catalog-graphql` | Yes (`SOURCE.md`) | `gdcorp-commerce/catalog-api-v2` |
| `orders-graphql` | Yes (`SOURCE.md`) | `gdcorp-commerce/order-api` |
| `taxes/graphql` | Yes (`SOURCE.md`) | Tax subgraph + overlays |
| `domains-v3` | **No SOURCE.md** | Effectively `domains.domain-lifecycle-specification` |
| Most REST commerce specs | **No SOURCE.md** | Matching `commerce.*-specification` repos |
| `openapi-specs/specs/external/*` | Migrated from developer.godaddy.com | Overlaps older public API surface / `api-spec` era |

### Takeaway

The portal is **already structured for SSOT + sync**:

- Registry + generate pipeline is solid.
- What’s missing is **automated, versioned ingest** from specification repos
  (and clear `SOURCE.md` / pin for every registry entry).

Today the portal often **behaves like a second SSOT** because humans commit
updated YAML trees under `openapi-specs/specs/`.

---

## Part 4 — How the pieces link today (as-is)

```text
┌──────────────────────────────────────────────────────────────────────────┐
│ gdcorp-platform                                                          │
│                                                                          │
│  domains.domain-lifecycle-specification   ◄── humans edit Domains v3     │
│  commerce.businesses-specification        ◄── humans edit Businesses     │
│  commerce.channels-specification          …                              │
│  apis.webhooks-specification              …                              │
│  api-spec                                 ◄── review + legacy Swagger    │
│         ▲                                                                │
│         │ submodule                                                      │
│  api-specification-aggregator  (discovery only)                          │
└────────────────────────────┬─────────────────────────────────────────────┘
                             │
                             │  manual copy / occasional PR
                             │  (GraphQL: documented gh api refresh)
                             ▼
┌──────────────────────────────────────────────────────────────────────────┐
│ gdcorp-commerce/developer-ecosystem-documentation                        │
│                                                                          │
│  openapi-specs/specs/*     ◄── vendored copies (second SSOT risk)        │
│  openapi-specs/registry.ts ◄── generation wiring                         │
│  scripts/bundle-specs.ts   ◄── portal transforms                         │
│  npm run generate          ◄── MDX under content/docs/references/        │
│  content/docs/api-users/   ◄── hand-authored guides (link to generated)  │
└──────────────────────────────────────────────────────────────────────────┘
                             │
                             ▼
                    developer.godaddy.com (docs site)
```

**Parallel consumers** (also need pinning):

| Consumer | How it gets specs today |
|----------|-------------------------|
| `godaddy/cli` `generate-api-catalog` | Discovers/clones platform repos / hardcoded lists |
| `godaddy/cli` `domains-client` | Bundled Domains v3 OpenAPI in CLI repo |
| Service implementation repos | Own copies / codegen — varies by team |

---

## Part 5 — How SSOT + Downstream Sync solves this (mapped to real repos)

### Corrected principle

> **One editable contract per capability in its `*-specification` repo.**  
> Everything else is a **pinned, automated derivative** of that repo’s release.

| Artifact | Editable? | Source of truth |
|----------|-----------|-----------------|
| `*-specification` OpenAPI (`v{N}/schemas/`) | **Yes** | Capability team |
| `api-spec` | Review / legacy only | Not Domains v3 write path |
| Docs `openapi-specs/specs/` | **No** (bot only) | Sync from specification release pin |
| Docs generated MDX | **No** | `npm run generate` |
| Docs portal transforms | Yes (adapter only) | Docs repo scripts |
| Hand-authored `api-users/` | Yes | Docs writers |
| CLI / SDK generated clients | **No** | Pin + regen |

### Target flow (Domains v3 — federated repo)

```text
1. Author edits:
     gdcorp-platform/domains.domain-lifecycle-specification
       v3/schemas/openapi.yaml (+ models)

2. PR gates on that specification repo:
     Spectral lint
     oasdiff vs previous release
     team CODEOWNERS

3. Merge → Publish from THAT repo only:
     GitHub Release (e.g. v1.4.2) + OpenAPI artifact + checksum
     Notify hardcoded consumers via reusable workflow:
       - developer-ecosystem-documentation
       - godaddy/cli

4. Docs sync workflow (one workflow, many packageIds):
     Pull artifact; verify sha256
     Replace openapi-specs/specs/domains-v3/** only
     Update pins.json domains/v3 → 1.4.2
     bundle-specs + generate → PR

5. CLI sync workflow:
     Same dispatch → pin + Domains codegen → PR

6. Humans / auto-merge → portal deploy / CLI release
```

### What stays where

#### In each `*-specification` (SSOT)

- OpenAPI under `v{N}/schemas/`
- `common-types` submodule pin
- Spectral + oasdiff CI
- **Release + notify workflow** (call reusable workflow from template)
- Release notes / user guide

#### In `api-specification-template`

- Bootstrap layout for new APIs
- **Reusable** `release-and-notify` workflow + caller stub so new repos inherit notify

#### In `api-spec`

- Architecture review + legacy Swagger/OpenAPI — unchanged role

#### In `developer-ecosystem-documentation` / `godaddy/cli`

- `pins.json`, mapping `packageId` → paths
- One sync workflow each (`repository_dispatch`)
- Bot-only vendored specs / generated clients

#### In `api-specification-aggregator`

- Discovery only; manually `git submodule add` new specification repos

---

## Part 5b — Workflows: who needs what?

### Yes — each publishing `*-specification` needs a workflow

That workflow:

1. Builds/bundles OpenAPI on merge/tag.
2. Creates GitHub Release + asset.
3. **Notifies** the hardcoded consumer list (`repository_dispatch`).

Avoid N copy-pasted YAML files: implement once as a **reusable workflow**, ship
the caller in `api-specification-template`, enable on Domains first, then roll
out.

### Consumers need workflows too (but not one-per-API)

| Repo | Needs |
|------|--------|
| Every `*-specification` that drives docs/CLI | Release + notify (publisher) |
| `developer-ecosystem-documentation` | **One** sync workflow for all `packageId`s |
| `godaddy/cli` | **One** sync workflow for APIs it uses |

### Hardcoded consumers (&lt;3)

```text
notify gdcorp-commerce/developer-ecosystem-documentation
notify godaddy/cli
# optional third service
```

Keep the list inside the reusable workflow so all specification repos stay aligned.

### Pin shape

```json
{
  "domains/v3": {
    "sourceRepo": "gdcorp-platform/domains.domain-lifecycle-specification",
    "version": "1.4.2",
    "tag": "v1.4.2",
    "sha256": "…"
  }
}
```

Still required: notify ≠ accepted version.

### Why federated (not monorepo) for this design

| Concern | Federated `*-specification` (preferred) |
|---------|----------------------------------------|
| Isolation of releases | Natural — separate repos |
| How new APIs are created | Already: GitHub template |
| Discoverability | Aggregator submodules |
| Shared types | `common-types-specification` submodule |
| Consumer sync | Same pin PR pattern either way |
| Blast radius | Only the releasing repo notifies; only that pin updates |

Monorepo consolidation is **out of scope** / non-goal.

---

## Part 6 — Gap analysis: design vs current repos

| Design requirement | Current state | Gap |
|--------------------|---------------|-----|
| Editable OpenAPI per capability | Exists as `*-specification` | Keep; do not migrate to monorepo |
| Versioned publish per API | Mostly tip-of-branch / informal | Release artifact + tag on each repo |
| Notify docs/CLI | Manual copy | Release workflow notify (reusable) |
| Automated docs sync | Manual vendoring | Docs `repository_dispatch` + pins |
| Breaking-change gate | Uneven | Spectral + oasdiff on each spec repo |
| Portal transforms | Present | Keep as adapter |
| Provenance | Sparse `SOURCE.md` | `pins.json` everywhere |
| Drift detection | Human | Weekly pin vs latest release |

---

## Part 7 — Implementation work breakdown (repo-specific)

### Phase A — Inventory & provenance (1–2 weeks)

| ID | Task | Repo |
|----|------|------|
| A.1 | Document upstream repo per `registry.ts` entry (`*-specification` vs `api-spec` vs unknown) | docs |
| A.2 | Flag any dual-sourced keys; pick one upstream | Platform + Docs |
| A.3 | Add `pins.json` for Domains v3 (+ others as rolled out) | docs |
| A.4 | Diff portal `domains-v3` vs Domains specification tip | Platform + Docs |
| A.5 | Confirm template → new repo process; document `api-spec` review vs SSOT roles | Platform |

### Phase B — Publisher workflow (Domains first) (2–4 weeks)

| ID | Task | Repo |
|----|------|------|
| B.1 | Spectral + oasdiff on Domains specification | `domains.domain-lifecycle-specification` |
| B.2 | Release workflow: artifact + tag + checksum | Domains |
| B.3 | Hardcoded notify → docs + CLI | Domains |
| B.4 | Extract reusable workflow; add caller to template | `api-specification-template` |
| B.5 | Roll reusable caller to next APIs as needed | other `*-specification` |

**Verification:** Domains merge → release → dispatch to docs/CLI.

### Phase C — Docs + CLI sync (3–5 weeks)

| ID | Task | Repo |
|----|------|------|
| C.1 | Docs `sync-api-spec.yml` + mapping for `domains/v3` | docs |
| C.2 | Pin update + generate + PR | docs |
| C.3 | Branch protection on `openapi-specs/specs/` | docs |
| C.4 | CLI pin + codegen sync workflow | `godaddy/cli` |
| C.5 | E2E pilot Domains → portal + CLI PRs | all |

**Verification:** Domains tag → docs PR + CLI PR; other pins unchanged.

### Phase D — Drift + rollout (ongoing)

| ID | Task | Repo |
|----|------|------|
| D.1 | Weekly drift: pins vs latest releases | Platform |
| D.2 | **Backport** thin release+notify caller to more existing `*-specification` repos (portal-backed first) | Platform |
| D.2a | Scripted batch PRs for caller workflow (manifest from package-map / inventory) | Platform |
| D.3 | New template-created repos include caller by default | template |

---

## Part 8 — Answers

| Question | Answer |
|----------|--------|
| Keep individual `*-specification` repos? | **Yes** — preferred SSOT. |
| Does each need a workflow to update portal/CLI? | **Yes** — release + notify (share via reusable workflow). Docs/CLI each need **one** sync workflow. |
| Migrate into `api-spec` monorepo? | **No** for this design. |
| Does Domains release force Shoppers update? | **No.** |
| How does portal consume? | Dispatch → pull OpenAPI artifact → replace that path + pin → generate → PR. |
| Same OpenAPI file? | Yes — copy published OpenAPI; no Swagger conversion. |
| Is there drift today? | **Yes** — Domains tip ahead of docs sync. |
| How are repos created? | [`api-specification-template`](https://github.com/gdcorp-platform/api-specification-template). |
| What does `api-spec` review? | Exposure-based architecture/API-design review + legacy catalog — not Domains v3 SSOT. |
| Copy from both `api-spec` and `*-specification`? | **No** for the same API. One pin → one upstream. |

---

## Part 9 — Recommended messaging for stakeholders

**One-liner:**

> Capability teams keep OpenAPI in their `*-specification` repo; each repo
> publishes a versioned artifact and notifies the developer portal and CLI;
> those consumers pin and sync through automation — never hand-edited copies.

**What we stop doing:**

- Manually copying OpenAPI into `developer-ecosystem-documentation`
- Treating the docs repo as an editable contract
- Planning an `api-spec` monorepo migration for modern APIs

**What we keep:**

- Federated `*-specification` repos + template bootstrap
- Architecture review in `api-spec` where required
- Aggregator for cross-capability search
- Docs generate pipeline and transforms
- `pins.json` + opt-in pin PRs

---

## Related documents

- [API Spec SSOT Architect One-Pager](./API_SPEC_SSOT_ARCHITECT_ONEPAGER.md) — shareable summary for architecture review
- [CLI work breakdown (Domains pilot)](./API_SPEC_SSOT_CLI_WORK_BREAKDOWN.md) — `godaddy/cli` + Domains `*-specification` tasks
- [API Spec SSOT + Downstream Sync (design)](./API_SPEC_SSOT_DOWNSTREAM_SYNC.md) — architecture; federated `*-specification` SSOT + notify/pin sync
- [API Spec SSOT Implementation Walkthrough](./API_SPEC_SSOT_IMPLEMENTATION_WALKTHROUGH.md) — publish → notify → pin PR; workflows per spec repo
- Upstream: [api-specification-template](https://github.com/gdcorp-platform/api-specification-template) (bootstrap)
- Upstream: [api-spec README](https://github.com/gdcorp-platform/api-spec/blob/master/README.md) (review/legacy)
- Upstream: [domains.domain-lifecycle-specification](https://github.com/gdcorp-platform/domains.domain-lifecycle-specification)
- Upstream: [developer-ecosystem-documentation](https://github.com/gdcorp-commerce/developer-ecosystem-documentation)
- Upstream: [api-specification-aggregator](https://github.com/gdcorp-platform/api-specification-aggregator)

---

## Appendix — Domains v3 link map (concrete)

| Concern | Location (today) | Preferred |
|---------|------------------|-----------|
| Editable OpenAPI | `domains.domain-lifecycle-specification` → `v3/schemas/openapi.yaml` | Same repo (SSOT) |
| Release | Ad hoc / tip of branch | GitHub Release + artifact from that repo |
| Notify | None | Reusable workflow → docs + CLI dispatch |
| Docs vendored copy | `openapi-specs/specs/domains-v3/` | Same path; bot-synced from pin |
| Registry entry | `openapi-specs/registry.ts` key `domains-v3` | Unchanged |
| Pin | Missing / informal | `openapi-specs/pins.json` → `domains/v3` |
| Bundled artifact | `openapi-specs/bundled/domains-v3.json` | Unchanged pipeline |
| Generated reference | `content/docs/references/rest/domains/v3/` | Unchanged |
| Hand-authored guides | `content/docs/api-users/domains/` | Unchanged |
| Portal transforms | `PORTAL_TRANSFORMS['domains-v3']` | Unchanged (adapter) |
| CLI client | `godaddy/cli` → `rust/domains-client` | Pin + regen from Domains release |
| Legacy Domains in api-spec | `spec/v1/…`, `spec2/…` (not v3 lifecycle) | Keep as legacy |
