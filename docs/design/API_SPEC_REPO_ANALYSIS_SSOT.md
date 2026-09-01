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

**Consolidate into `api-spec` as a monorepo**, with **product → API version
folders** and **independent releases per version tree**. A Domains v3 change
must **not** bump or republish Domains v2, Shoppers, Hosting, or any other API.

```
api-spec/                              ◄── single repo, many API packages
  apis/
    domains/
      v2/                              ◄── release: domains/v2@…
      v3/                              ◄── release: domains/v3@1.4.2
    shoppers/
      v1/                              ◄── release: shoppers/v1@2.1.0
    certificates/
      v1/
  …
        │
        │ path-filtered CI: only changed version trees release
        ▼
  GitHub Release / package per apis/<product>/<version>/
        │
        ├─► developer-ecosystem-documentation  (pins domains/v3@1.4.2 only)
        ├─► gddy CLI / SDKs                    (pin per product+version)
        └─► service implementation repos
```

Prefer **`apis/domains/v3/`** over a flat `domains-v3/` folder: the product is
the ownership unit; `v3` is the public API generation. Flat ids like
`domains-v3` remain fine as **registry/pin keys** derived from that path.

Separate `*-specification` repos are the **current** state of the world; they
are a migration source, not the preferred end state. The aggregator becomes
unnecessary once everything lives under `api-spec/apis/`.

See [Per-API versioning & consumer consumption](#part-5b--preferred-target-api-spec-monorepo--per-api-releases)
for how releases work and how the portal / CLI pull them.

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

**Today:** Treat `api-spec` as (1) the architecture review gate and (2) a legacy
catalog — modern Domains v3–style contracts live elsewhere.

**Preferred end state:** Expand `api-spec` into the **monorepo SSOT**
(`apis/<product>/<version>/…`) with **independent releases per version tree**,
while keeping the review process. Separate `*-specification` repos become
migration sources that fold into those folders under `api-spec`.

---

## Part 2 — Modern SSOT today: `*-specification` repos

> **Status:** this is the **current** layout. The preferred target consolidates
> these into folders under `api-spec` (see Part 5b). Keep this section as the
> inventory of what must be migrated.

### Pattern (as-is)

Platform teams currently own dedicated repos named:

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

**Today** the editable OpenAPI for a modern Published API is the matching
`*-specification` repo. **Preferred** is to migrate each tree into
`api-spec/apis/<product>/<version>/` (e.g. Domains Lifecycle →
`apis/domains/v3/`) and retire the separate repo after cutover. The aggregator
is only needed while federation lasts; a monorepo checkout replaces it.

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

> **One editable contract per product + API version, co-located under `api-spec`.**  
> Everything else is a **pinned, automated derivative** of that version tree’s release.

| Artifact | Editable? | Source of truth |
|----------|-----------|-----------------|
| `api-spec/apis/<product>/<version>/` OpenAPI | **Yes** | Capability team (CODEOWNERS on the product) |
| Legacy `*-specification` (during migration) | Yes until cutover | Same team; then archive |
| Docs `openapi-specs/specs/` | **No** (bot only) | Sync from that version’s release pin |
| Docs generated MDX | **No** | `npm run generate` |
| Docs portal transforms | Yes (adapter only) | Docs repo scripts |
| Hand-authored `api-users/` | Yes | Docs writers |
| CLI / SDK generated clients | **No** | Pin + regen per product/version |

### Target flow (Domains v3 example — preferred monorepo)

```text
1. Author edits:
     gdcorp-platform/api-spec/apis/domains/v3/openapi.yaml  (+ models)

2. PR gates (path-filtered to apis/domains/v3/**):
     Spectral lint
     oasdiff vs previous domains/v3 release (breaking → package major)
     CODEOWNERS (Domains Platform — owns apis/domains/)

3. Merge to main → Publish ONLY domains/v3:
     Tag / GitHub Release: domains/v3@1.4.2
     Artifact: openapi.yaml tree + checksum + CHANGELOG for domains/v3 only
     (domains/v2, shoppers/v1, … are untouched — no new versions)

4. Downstream sync bot (docs repo):
     Subscribes to api-spec releases filtered by package id
     Downloads domains/v3@1.4.2 artifact only
     Replaces openapi-specs/specs/domains-v3/**   # portal key may stay flat
     Updates pins.json:
       { "domains/v3": { "path": "apis/domains/v3", "version": "1.4.2", "sha256": "..." } }
     Runs: npm run bundle-specs && npm run generate
     Opens PR: "chore(specs): sync domains/v3@1.4.2"

5. Docs PR gates:
     lint:openapi, generate:check, build
     Auto-merge patch/minor; human review for major

6. Consumers (CLI / SDKs):
     Pin domains/v3@1.4.2 independently of domains/v2 or other products
     Regenerate only domains v3 client / catalog entries
     CI fails if regen diff without pin bump
```

Migration note: until Domains lands under `api-spec/apis/domains/v3/`, the same
release + pin flow can run against `domains.domain-lifecycle-specification`;
only the source path and release publisher change at cutover.

### What stays where (preferred end state)

#### In `api-spec` (SSOT monorepo)

- `apis/<product>/<version>/` — OpenAPI (+ models) per public API generation
- Path-filtered CI + publish workflow per version tree
- CODEOWNERS per `apis/<product>/` (covers all versions for that product)
- Architecture review process (unchanged intent; applied to the same folders)
- Legacy `spec/` / `spec2/` / `spec3/` until migrated or archived

#### In `developer-ecosystem-documentation` (downstream)

- Keep `registry.ts`, bundle transforms, fumadocs generation
- Add `pins.json` (or `SOURCE.md`) for **every** registry entry
- Make `openapi-specs/specs/**` bot-writable only
- Keep hand-authored guides; they **link** to generated reference
- Portal keys may stay flat (`domains-v3`) while mapping to `apis/domains/v3`

#### Retired / optional after monorepo

- Separate `*-specification` repos → archive after import
- `api-specification-aggregator` → optional; monorepo checkout replaces discovery

---

## Part 5b — Preferred target: `api-spec` monorepo + per-version releases

### Why not one repo-wide release?

A single `api-spec@2.0.0` that republishes every API whenever Domains v3 changes
would force:

- Spurious version bumps for unrelated consumers
- Docs/CLI churn for APIs that did not change
- Ambiguous changelogs (“what actually moved?”)

**Rule:** version and publish **per `apis/<product>/<version>/` tree**, never
the whole repo as one artifact (except an optional discovery index that lists
latest pins).

### Layout: product → version (preferred over flat `domains-v3/`)

```text
api-spec/
  apis/
    domains/                    # product / ownership unit
      v2/                       # older public generation (if still published)
        openapi.yaml
        CHANGELOG.md
        package.json            # name: @godaddy/api-spec-domains-v2
      v3/                       # current Domains Lifecycle API
        openapi.yaml            # or schemas/openapi.yaml (preserve import layout)
        models/
        CHANGELOG.md            # domains/v3 only
        package.json            # name: @godaddy/api-spec-domains-v3, version: 1.4.2
    shoppers/
      v1/
        …
    certificates/
      v1/
        …
  packages/                     # optional shared publish helpers
  .github/workflows/
    validate-api.yml            # path filters: apis/<product>/<version>/**
    release-api.yml             # releases only changed version trees
  CODEOWNERS                    # /apis/domains/ @domains-platform
  catalog.json                  # optional: product/version → latest package semver
```

**Why this beats flat `apis/domains-v3/`:**

| Concern | Flat `domains-v3/` | Nested `domains/v3/` |
|---------|--------------------|----------------------|
| Browse by product | Harder (v2/v3 scattered) | Natural |
| CODEOWNERS | One rule per flattened id | One rule for `/apis/domains/` |
| Shipping Domains v4 later | New sibling folder at top | New `domains/v4/` next to `v3/` |
| Shared Domains notes | Awkward | `domains/README.md` |

Flat strings like `domains-v3` are still useful as **pin/registry keys**
(`"domains/v3"` or `"domains-v3"` → same publish unit).

### Two version axes (do not conflate)

| Axis | Meaning | Example |
|------|---------|---------|
| **API version** (folder) | Public contract generation / URL family | `v3` in `apis/domains/v3/` |
| **Spec package semver** (release) | Revisions of that OpenAPI document | `domains/v3@1.4.2` — additive field → `1.5.0`; breaking doc change inside v3 → `2.0.0` of the **package**, still living under folder `v3` until you open `v4/` |

Breaking the **HTTP API** in a way that needs a new public generation → new
folder `apis/domains/v4/`, not only a package major under `v3`.

### How a Domains v3-only change releases (path-filtered)

1. PR touches only `apis/domains/v3/**`.
2. CI runs Spectral + oasdiff **only** for `domains/v3`.
3. On merge, release job detects changed package(s) via path filter or
   `package.json` version bump (Changesets / release-please / custom script).
4. Publishes **`domains/v3@1.4.2`** only:
   - GitHub Release tag: `domains/v3@1.4.2`
   - Release asset: `domains-v3-1.4.2.tgz` (bundled OpenAPI + checksum)
   - Optional: npm/GitHub Packages `@godaddy/api-spec-domains-v3@1.4.2`
5. `apis/domains/v2/`, `apis/shoppers/v1/`, etc. keep previous versions. No new tags for them.

**Version bump ownership:** the PR that changes Domains v3 OpenAPI must bump
`apis/domains/v3` package version (enforced by CI). Unrelated folders stay frozen.

### How consumers get a release after publish

Consumers never clone “whatever is on main.” They **pin a package version**.

```text
                    domains/v3@1.4.2 published
                              │
          ┌───────────────────┼───────────────────┐
          ▼                   ▼                   ▼
   Developer portal      gddy CLI / SDKs     Service / codegen
   (docs repo)           (godaddy/cli)       (implementation)
```

#### 1. Developer portal (`developer-ecosystem-documentation`)

| Step | What happens |
|------|----------------|
| Trigger | GitHub `release` webhook / `repository_dispatch` for package `domains/v3` |
| Fetch | Download `domains-v3-1.4.2` artifact (or `npm pack` equivalent) |
| Write | Replace `openapi-specs/specs/domains-v3/**` only (portal path can stay flat) |
| Pin | Update `openapi-specs/pins.json` → `"domains/v3": "1.4.2"` (+ sha256) |
| Generate | `npm run bundle-specs && npm run generate` → MDX under `content/docs/references/rest/domains/v3/` |
| Ship | Bot opens PR; merge → portal deploy shows new Domains v3 reference |

Shoppers docs and Domains v2 pins are unchanged until those packages publish.

#### 2. CLI / SDK consumers

| Step | What happens |
|------|----------------|
| Pin file | e.g. `rust/schemas/api/pins.json` → `"domains/v3": "1.4.2"` |
| Fetch | CI/script downloads that exact artifact |
| Codegen | Regenerate `domains-client` / API catalog entries for Domains v3 only |
| Gate | `cargo test` / regen-check fails if generated files drift without pin bump |

Developers upgrade Domains v3 by bumping **one pin**, not by re-vendoring the
whole monorepo.

#### 3. Service implementation teams

Same pin pattern: lockfile or `api-spec.pin` points at `domains/v3@1.4.2`.
Contract tests / oasdiff run against that artifact in CI.

#### 4. Optional registry index

`catalog.json` lists latest package semver per `product/version` so humans and
bots can discover “what’s current?” without treating monorepo `main` as an
unversioned free-for-all.

### Pin file shape (docs + CLI)

```json
{
  "domains/v3": {
    "path": "apis/domains/v3",
    "package": "@godaddy/api-spec-domains-v3",
    "version": "1.4.2",
    "source": "gdcorp-platform/api-spec",
    "tag": "domains/v3@1.4.2",
    "sha256": "…"
  },
  "shoppers/v1": {
    "path": "apis/shoppers/v1",
    "package": "@godaddy/api-spec-shoppers-v1",
    "version": "2.1.0",
    "tag": "shoppers/v1@2.1.0",
    "sha256": "…"
  }
}
```

Each consumer bumps **only the keys it cares about**.

### Comparison: federated repos vs monorepo folders

| Concern | Many `*-specification` repos (today) | `api-spec` `product/version` + per-tree release (preferred) |
|---------|--------------------------------------|--------------------------------------------------|
| Isolation of releases | Natural (separate repos) | Achieved via path filters + per-tree tags |
| Discoverability | Needs aggregator / many remotes | One clone; browse `apis/<product>/` |
| Multiple API generations | Separate repos or awkward names | Sibling folders `v2/`, `v3/`, `v4/` |
| Cross-API shared types | Submodules / copy | Shared folder + explicit deps, still versioned |
| Review process | Split from `api-spec` | Same repo as review gate |
| Consumer pins | One pin per remote | One pin per `product/version` (same UX) |
| Blast radius of Domains v3 change | Only Domains repo releases | Only `domains/v3` package releases |

**Bottom line:** nested folders do **not** imply one shared version. Publish
granularity is per `apis/<product>/<version>/`, same isolation as separate repos
without flattening product and version into a single directory name.

## Part 6 — Gap analysis: design vs current repos

| Design requirement | Current state | Gap |
|--------------------|---------------|-----|
| Single editable OpenAPI per product/version under `api-spec` | Modern APIs in separate `*-specification` repos | Migrate into `api-spec/apis/<product>/<version>/` |
| Independent per-version-tree releases | No standard publish; repo tip ≠ consumer pin | Path-filtered release + tags like `domains/v3@1.4.2` |
| Automated docs sync | Manual vendoring; GraphQL has refresh scripts | Bot + pins per product/version package |
| Breaking-change gate at source | Partial (`api-spec` review; uneven on `*-specification`) | Spectral + oasdiff on each `apis/<product>/<version>/` |
| Portal transforms | Present and correct | Keep; document as adapter |
| Provenance | Only some GraphQL `SOURCE.md` | `pins.json` for every registry entry |
| Consumer pinning | CLI has its own copies | Pin per API package version |
| Drift detection | Accidental / human | Weekly job: pin vs latest package release |

---

## Part 7 — Implementation work breakdown (repo-specific)

### Phase A — Inventory & provenance (1–2 weeks)

| ID | Task | Repo |
|----|------|------|
| A.1 | For each `openapi-specs/registry.ts` entry, document upstream + last sync | docs |
| A.2 | Add `pins.json` / `SOURCE.md` for Domains v3 and commerce REST specs | docs |
| A.3 | Diff `domains-v3` vs current upstream tip; file drift report | Platform + Docs |
| A.4 | Map Published APIs: `api-spec` only vs `*-specification` vs both | Platform |
| A.5 | Decide folder naming (`apis/<product>/<version>/`) and migration order | Platform |

**Verification:** Every registry `key` has a resolvable upstream URL and pin field.

### Phase B — Monorepo layout + per-API publish (3–6 weeks)

| ID | Task | Repo |
|----|------|------|
| B.1 | Create `apis/` layout + CODEOWNERS + path-filtered Spectral/oasdiff | `api-spec` |
| B.2 | Release workflow: only changed API packages get tags/artifacts | `api-spec` |
| B.3 | Pilot: import Domains Lifecycle v3 into `apis/domains/v3/` | `api-spec` + Domains |
| B.4 | Publish `domains/v3@x.y.z` from monorepo; keep old repo read-only mirror briefly | Platform |
| B.5 | Migrate next 2–3 high-traffic APIs; template for remaining | Platform |

**Verification:** Domains-only PR produces `domains/v3@…` only; other package versions unchanged.

### Phase C — Docs downstream sync bot (3–5 weeks)

| ID | Task | Repo |
|----|------|------|
| C.1 | `sync-spec.yml`: on per-API release, pull artifact into `openapi-specs/specs/<key>/` | docs |
| C.2 | Update pin; run `bundle-specs` + `generate`; open PR | docs |
| C.3 | Branch protection: only bot + admins can modify `openapi-specs/specs/` | docs |
| C.4 | Preserve portal transforms in `bundle-specs.ts` | docs |
| C.5 | Pilot Domains v3 end-to-end from `api-spec` release | Domains + Docs |

**Verification:** Tag `domains/v3@…` → docs PR within 1 hour → site shows new operation; Shoppers pin unchanged.

### Phase D — Consumers + drift (ongoing)

| ID | Task | Repo |
|----|------|------|
| D.1 | CLI pins `domains/v3` package version; CI regen check | `godaddy/cli` |
| D.2 | Weekly job: compare pins vs latest package releases; Slack on drift | Platform |
| D.3 | Policy: no hand PR that only edits vendored YAML without pin bump | docs |
| D.4 | Archive migrated `*-specification` repos; retire aggregator if unused | Platform |

---

## Part 8 — Answers to “does the design work with these repos?”

| Question | Answer |
|----------|--------|
| Should every API keep its own GitHub repo? | **Preferred: no** — `apis/<product>/<version>/` under `api-spec` with **per-tree** version tags/artifacts. |
| Does a Domains v3 change force releasing other APIs? | **No** — path-filtered CI + package-scoped releases (`domains/v3@1.4.2` only; Domains v2 untouched). |
| How does the developer portal consume a release? | Bot downloads that tree’s artifact → updates `openapi-specs/specs/<key>/` + `pins.json` → `bundle-specs` / `generate` → docs PR → deploy. |
| How do CLI / SDKs consume a release? | Pin `domains/v3@…` in a lockfile; fetch artifact; regen only that client/catalog slice. |
| Does the docs portal already support generated references? | **Yes** — registry, bundle, generate, lint:openapi are production-ready. |
| What’s the main missing piece today? | Monorepo import + **per product/version** publish + automated pin sync (today: federated repos + manual vendoring). |
| Should docs stop transforming specs? | **No** — keep portal transforms as an adapter layer. |
| Is Domains v3 in `api-spec` today? | **No** — migrate from `domains.domain-lifecycle-specification` into `apis/domains/v3/`. |
| Flat `domains-v3` vs nested `domains/v3`? | **Prefer nested** for layout; flat keys OK for portal registry/pins. |
| Is there already drift? | **Yes** — Domains tip (2026-08-28) ahead of docs sync (2026-08-14). |

---

## Part 9 — Recommended messaging for stakeholders

**One-liner:**

> Capability teams edit OpenAPI under `api-spec/apis/<product>/<version>/`;
> each version tree publishes its own artifact; the developer portal and CLI
> pin and sync those packages — never hand-edit copies, and never version the
> whole repo as one blob.

**What we stop doing:**

- Manually copying OpenAPI trees into `developer-ecosystem-documentation`
- Treating the docs repo as an editable contract
- Spawning a new GitHub repo per API when a folder + CODEOWNERS would do
- Flattening product and API version into a single top-level folder name when
  nested `product/version` is clearer
- Repo-wide releases that republish every API

**What we keep:**

- Architecture review intent (now applied to the same monorepo folders)
- Docs portal generation pipeline and transforms
- Hand-authored guides that link to generated reference
- Independent versioning per product/API-version tree
---

## Related documents

- [API Spec SSOT + Downstream Sync (design)](./API_SPEC_SSOT_DOWNSTREAM_SYNC.md) — architecture; this analysis maps GoDaddy repos and prefers the monorepo + per-API release model
- Upstream: [api-spec README](https://github.com/gdcorp-platform/api-spec/blob/master/README.md)
- Upstream: [domains.domain-lifecycle-specification](https://github.com/gdcorp-platform/domains.domain-lifecycle-specification) (migration source)
- Upstream: [developer-ecosystem-documentation](https://github.com/gdcorp-commerce/developer-ecosystem-documentation)
- Upstream: [api-specification-aggregator](https://github.com/gdcorp-platform/api-specification-aggregator) (temporary while federated)

---

## Appendix — Domains v3 link map (concrete)

| Concern | Location (today) | Preferred |
|---------|------------------|-----------|
| Editable OpenAPI | `domains.domain-lifecycle-specification` → `v3/schemas/openapi.yaml` | `api-spec/apis/domains/v3/` |
| Release | Ad hoc / tip of branch | GitHub/npm package `domains/v3@x.y.z` |
| Docs vendored copy | `openapi-specs/specs/domains-v3/` | Same path; bot-synced from pin |
| Registry entry | `openapi-specs/registry.ts` key `domains-v3` | Unchanged (flat key → nested source) |
| Pin | Missing / informal | `openapi-specs/pins.json` → `domains/v3` version |
| Bundled artifact | `openapi-specs/bundled/domains-v3.json` | Unchanged pipeline |
| Generated reference | `content/docs/references/rest/domains/v3/` | Unchanged |
| Hand-authored guides | `content/docs/api-users/domains/` | Unchanged |
| Portal transforms | `PORTAL_TRANSFORMS['domains-v3']` | Unchanged (adapter) |
| CLI client | `godaddy/cli` → `rust/domains-client` | Pin `domains/v3@x.y.z` + regen |
| Legacy Domains in api-spec | `spec/v1/domains.json`, `spec2/…` (not v3 lifecycle) | Keep until archived / moved under `apis/domains/v1` etc. |
