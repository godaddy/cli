# Walkthrough: Implementing API Spec SSOT + Downstream Sync

This document is an **implementation-oriented walkthrough**: who does what, where
versions live, how the developer portal and other consumers stay in sync, and
whether sync is push or pull.

It assumes the preferred architecture from:

- [API_SPEC_SSOT_DOWNSTREAM_SYNC.md](./API_SPEC_SSOT_DOWNSTREAM_SYNC.md)
- [API_SPEC_REPO_ANALYSIS_SSOT.md](./API_SPEC_REPO_ANALYSIS_SSOT.md)

**Target shape (decided):** keep the existing federated SSOT —

`gdcorp-platform/<domain>.<capability>-specification`

(bootstrapped from [`api-specification-template`](https://github.com/gdcorp-platform/api-specification-template)).
Each specification repo **publishes its own versioned OpenAPI artifact** and
runs a **GitHub workflow that notifies a small hardcoded list of consumers**
(developer portal, `godaddy/cli`, optionally one service). Consumers never treat
git tip as the contract — they **pin a published release**.

Do **not** migrate into an `api-spec` monorepo for this design. `api-spec`
remains the architecture-review / legacy Swagger store.

---

## 1. Mental model (read this first)

### Three different “versions”

| Name | What it is | Example |
|------|------------|---------|
| **API version** | Public HTTP generation / folder in the spec repo | `v3/` in `domains.domain-lifecycle-specification` |
| **Spec package version** | Semver (or tag) of the OpenAPI *document* | `1.4.2` → tag `v1.4.2` or `domains-v3@1.4.2` |
| **Consumer pin** | What a downstream repo has *accepted* | Docs `pins.json`: `"domains/v3": "1.4.2"` |

Publishing a Domains release does **not** automatically change the live portal
or CLI. A consumer only moves when its **pin** is updated (usually via an
automated PR that a human or auto-merge policy accepts).

### Who owns what

| Actor | Owns | Does **not** do |
|-------|------|-----------------|
| Capability team (e.g. Domains) | Edit OpenAPI in their `*-specification` repo | Hand-edit docs portal YAML |
| Spec-repo release workflow | Publish immutable artifact + tag; **notify hardcoded consumers** | Merge PRs in consumer repos |
| Docs / CLI sync workflow | On notify: pull artifact, update pin + generated files, open PR | Invent a different OpenAPI |
| Docs / CLI maintainers | Review & merge pin-bump PRs (policy may auto-merge patch/minor) | Manually copy OpenAPI from Slack |
| Service implementation teams | Keep runtime compatible with **their** pin; bump when ready | Edit the SSOT OpenAPI in their service repo |

### Sync model in one sentence

**Push notification + pull artifact + pin PR (opt-in upgrade).**

- **Push:** the **specification repo** that released announces
  “`domains/v3@1.4.2` exists” (`repository_dispatch` to hardcoded consumers).
- **Pull:** each consumer downloads **that exact artifact**.
- **Pin PR:** consumer automation updates files + `pins.json` and opens a PR.

```text
  domains.domain-lifecycle-specification
  (or commerce.businesses-specification, …)
           │
           │ merge → GitHub Release + artifact
           │ PUSH: hardcoded notify (docs, cli, …)
           ▼
    ┌──────────────┬──────────────┐
    ▼              ▼              ▼
 docs portal    godaddy/cli   (optional service)
 PULL artifact  PULL artifact
 pin + generate pin + codegen
 open PR        open PR
    │              │
    └──────┬───────┘
           ▼
    humans / auto-merge → deploy
```

### Do each `*-specification` repos need a workflow?

**Yes — each specification repo that should drive the portal/CLI needs a
release + notify workflow** (publish artifact, then `repository_dispatch` to
consumers).

That includes **existing** repos (Domains, businesses, channels, …), not only
new ones. Creating the workflow in the template alone does **not** update
repos that already exist — those must get a small **caller** workflow added
(or synced) in a rollout. See §1.1 below.

To avoid copy-paste across ~80 repos:

1. Put a **reusable workflow** in
   [`api-specification-template`](https://github.com/gdcorp-platform/api-specification-template)
   (or a small shared `api-specification-workflows` repo).
2. Each `*-specification` repo calls that reusable workflow with its
   `packageId` (e.g. `domains/v3`) and the **same hardcoded consumer list**.
3. **New** repos created from the template get the caller by default.
4. **Existing** repos: add the same thin caller in waves (pilot Domains →
   portal-backed APIs → rest as needed).

Consumers need workflows too:

| Repo | Workflow role |
|------|----------------|
| Each `*-specification` that drives docs/CLI | Release + notify (publisher) — **add/update on existing repos** |
| `developer-ecosystem-documentation` | Receive dispatch → replace that API’s specs + pin → generate → PR |
| `godaddy/cli` | Receive dispatch → pin + codegen → PR |

One docs/CLI workflow handles **all** specification repos (payload carries
`packageId` / mapping key). You do **not** need a separate docs workflow per API.

### 1.1 Existing `*-specification` repos: what to change

GitHub’s “template repository” only applies at **create** time. The ~80 repos
already cloned from
[`api-specification-template`](https://github.com/gdcorp-platform/api-specification-template)
will **not** automatically pick up new workflow files. Plan:

| Step | Action |
|------|--------|
| 1 | Land reusable `release-and-notify.yml` in the template (or shared workflows repo) |
| 2 | Pilot: add a thin caller under `.github/workflows/` on **Domains** only |
| 3 | Roll out the **same caller** to other repos that the portal/CLI actually pin (not necessarily all 80 on day one) |
| 4 | Optional: scripted PR across the org (“add sync caller”) for remaining repos when ready |
| 5 | Ensure **new** template-created repos include the caller so they never need a backport |

**Caller example** (one file per existing repo — only `package_id` changes):

```yaml
# .github/workflows/release.yml  (add to e.g. domains.domain-lifecycle-specification)
name: Release OpenAPI and notify consumers
on:
  push:
    branches: [main, develop]  # match that repo’s default
    paths:
      - "v3/schemas/**"          # or v1/schemas/** per repo
      - ".github/workflows/release.yml"

jobs:
  release:
    uses: gdcorp-platform/api-specification-template/.github/workflows/release-and-notify.yml@main
    with:
      package_id: domains/v3
    secrets: inherit
```

Repos that are **not** published to the portal/CLI can skip notify until they
are in scope. Inventory (Phase 0) decides which existing repos get the caller
in wave 1 vs later.

### 1.2 Best way to update existing repos (scripted backport)

**Recommended approach:** do **not** hand-edit ~80 repos. After the reusable
workflow exists and Domains is proven:

1. Maintain a **manifest** of targets (from Phase 0 inventory / `package-map.json`):
   `repo`, `package_id`, `paths` filter (e.g. `v3/schemas/**`).
2. Run a **backport script** that, for each repo, opens a PR adding only:

   ```text
   .github/workflows/release.yml   # thin caller → reusable workflow
   ```

3. Humans (or auto-merge for non-prod dry-run) merge per repo / CODEOWNERS.
4. New repos keep inheriting from the template so they never need the script.

**Why a script + PR (not force-push):**

| Approach | Verdict |
|----------|---------|
| Manual PR per repo | Fine for pilot (Domains); too slow for dozens |
| **Script opens PRs** (`gh api` / `gh pr create`) | **Best** — auditable, CODEOWNERS, CI runs |
| Direct push to default branch | Avoid — no review, breaks protected branches |
| Dependabot/Renovate | Wrong tool (dependency bumps, not add workflow) |
| Org ruleset “required workflow” | Can *require* a workflow name later; still need the caller file + `package_id` input per repo |

**Illustrative script shape** (Platform-owned; lives in template or a small
`api-specification-tooling` repo):

```bash
#!/usr/bin/env bash
# backport-release-caller.sh
# Usage: ./backport-release-caller.sh targets.csv
# CSV columns: repo,package_id,paths
# Example row: gdcorp-platform/domains.domain-lifecycle-specification,domains/v3,v3/schemas/**

set -euo pipefail
TEMPLATE_REF="${TEMPLATE_REF:-gdcorp-platform/api-specification-template/.github/workflows/release-and-notify.yml@main}"

while IFS=, read -r repo package_id paths; do
  [[ "$repo" =~ ^#|^$ ]] && continue
  work="/tmp/backport-$(basename "$repo")"
  rm -rf "$work"
  gh repo clone "$repo" "$work" -- --depth 1
  mkdir -p "$work/.github/workflows"
  cat > "$work/.github/workflows/release.yml" <<YAML
name: Release OpenAPI and notify consumers
on:
  push:
    branches: [main, develop]
    paths: [${paths}]
  workflow_dispatch:
jobs:
  release:
    uses: ${TEMPLATE_REF}
    with:
      package_id: ${package_id}
    secrets: inherit
YAML
  (
    cd "$work"
    git checkout -b chore/add-api-spec-release-caller
    git add .github/workflows/release.yml
    git commit -m "chore: add OpenAPI release+notify caller workflow"
    git push -u origin HEAD
    gh pr create --title "chore: add OpenAPI release+notify caller" \
      --body "Adds thin caller for reusable \`release-and-notify\` (SSOT sync). package_id=\`${package_id}\`."
  )
done < "$1"
```

**Rollout order (same as WBS):**

1. Phase 1 — hand-add caller on **Domains** only; prove release → docs/CLI PR.  
2. Phase 4 — script against **portal-backed** repos from `package-map.json`.  
3. Later — optional second pass for remaining `*-specification` repos.  
4. Never require all ~80 on day one.

**Permissions:** script needs a GitHub App/PAT that can push branches and open
PRs across `gdcorp-platform/*-specification` (and read the reusable workflow
repo).

**Documented in WBS:** Phase 1 (manual pilot), Phase 4 task **4.1a** (script +
manifest + batch PRs). See design doc Phase 4 and analysis D.2.

**Documented in WBS:** Phase 1 (Domains + reusable), Phase 4 (roll caller to
more existing `*-specification` repos). Same story in the design doc and
analysis Part 7.

---

## 2. End-to-end flow (happy path)

Concrete story: Domains adds an optional field to Domains Lifecycle v3.

### Step A — Author in SSOT

1. Engineer opens a PR in
   `gdcorp-platform/domains.domain-lifecycle-specification` changing
   `v3/schemas/**` (OpenAPI).
2. PR bumps the package version (e.g. `package.json` or release-please) —
   CI fails if OpenAPI changed without a version bump.
3. CI runs Spectral + oasdiff vs previous release.
4. CODEOWNERS / team review.
5. PR merges to the default branch (`main` / `develop` per repo).

### Step B — Publish from **that** specification repo

On merge, **that repo’s** release workflow:

1. Bundles/dereferences OpenAPI, writes `manifest.json` + checksums.
2. Creates GitHub Release (e.g. tag `v1.4.2` or `domains-v3@1.4.2`) with
   asset `domains-v3-1.4.2.tgz`.
3. Optionally publishes `@godaddy/api-spec-domains-v3@1.4.2`.
4. **Notifies hardcoded consumers** (see Step C).

Other specification repos are untouched — no Shoppers release, no monorepo
path filter required (isolation is per GitHub repo).

### Step C — Notify consumers (hardcoded, &lt;3 repos)

**How does the docs repo hear about `1.4.2`?** The Domains specification
repo’s release job **calls it**. Prefer hardcoding the &lt;3 consumers in the
**reusable** notify step so every spec repo shares one list:

```yaml
# Called from domains.domain-lifecycle-specification (and peers) after release
- name: Notify consumer repos
  env:
    GH_TOKEN: ${{ secrets.API_SPEC_CONSUMER_TOKEN }}
    PACKAGE_ID: domains/v3
    TAG: ${{ steps.release.outputs.tag }}
    ASSET_URL: ${{ steps.release.outputs.asset_url }}
    SHA256: ${{ steps.release.outputs.sha256 }}
    PACKAGE_VERSION: ${{ steps.release.outputs.version }}
  run: |
    notify() {
      local repo="$1"
      gh api --method POST "repos/${repo}/dispatches" \
        -f event_type='api-spec-release' \
        -f "client_payload[packageId]=${PACKAGE_ID}" \
        -f "client_payload[packageVersion]=${PACKAGE_VERSION}" \
        -f "client_payload[tag]=${TAG}" \
        -f "client_payload[assetUrl]=${ASSET_URL}" \
        -f "client_payload[sha256]=${SHA256}" \
        -f "client_payload[sourceRepo]=${{ github.repository }}"
    }

    notify gdcorp-commerce/developer-ecosystem-documentation
    notify godaddy/cli
```

Each consumer has:

```yaml
on:
  repository_dispatch:
    types: [api-spec-release]
  workflow_dispatch:
  schedule:
    - cron: "0 14 * * 1"  # optional drift sweep
```

Cross-org: one GitHub App / PAT that can dispatch (or open PRs) on those
consumer repos.

### Step D — Developer portal sync

1. Docs workflow starts from dispatch (payload includes `packageId=domains/v3`).
2. Job **pulls** the release asset; verifies `sha256`.
3. **Mapping table** (in docs repo) maps `domains/v3` →
   `openapi-specs/specs/domains-v3/` + registry key `domains-v3`.
   See **§5.1 Package map** — this is an explicit checked-in file, not magic.
4. **Replaces only that folder** with the OpenAPI from the artifact (no
   Swagger↔OpenAPI conversion — same OpenAPI bytes).
5. Updates `openapi-specs/pins.json` (schema: **§5.2**):

   ```json
   {
     "domains/v3": {
       "sourceRepo": "gdcorp-platform/domains.domain-lifecycle-specification",
       "version": "1.4.2",
       "tag": "v1.4.2",
       "sha256": "…",
       "syncedAt": "2026-09-02T12:00:00Z"
     }
   }
   ```

6. Runs `npm run bundle-specs && npm run generate`.
7. Opens PR: `chore(specs): sync domains/v3@1.4.2`.
8. Policy: auto-merge patch/minor; human review for major.
9. Merge → portal deploy →
   [developer.godaddy.com Domains v3](https://developer.godaddy.com/en/docs/references/rest/domains/v3).

**Where is the portal’s version recorded?** `openapi-specs/pins.json` on the
docs default branch (not the tip of the specification repo).

### Step E — CLI sync (same notify line)

1. CLI workflow receives the same dispatch.
2. Downloads artifact; updates `pins.json`; regenerates Domains client/catalog
   only.
3. Opens PR; CI; merge → next CLI release.

### Step F — Service repos (optional, often slower)

Same notify or Renovate on the package. Human merge when runtime matches the
new pin.

---

## 3. Push vs pull — precise answer

| Question | Answer |
|----------|--------|
| Who updates consumers? | **Each `*-specification` release workflow notifies**; **consumer workflows open PRs**; owners merge. |
| Push or pull? | **Hybrid:** push event from spec repo + pull artifact + opt-in pin PR. |
| Need a workflow per API repo? | **Yes** for publish+notify (share via reusable workflow / template). |
| Need a workflow per API on docs/CLI? | **No** — one sync workflow per consumer repo, keyed by `packageId`. |
| Still need `pins.json`? | **Yes** — hardcoded notify ≠ knowing which version a consumer accepted. |

---

## 4. How consumers learn a new version exists

### 4.1 Primary: hardcoded notify from the releasing specification repo

After `domains.domain-lifecycle-specification` publishes, it notifies docs +
CLI (snippet in Step C). Same pattern for `commerce.businesses-specification`,
etc. — each release only notifies; consumers ignore packages they don’t map.

### 4.2 Secondary: drift job (pull)

Weekly: compare consumer `pins.json` to latest GitHub Release on the
`sourceRepo` for each pin (or a small catalog). Opens/updates PR if behind.

### 4.3 Human awareness

Slack `#api-spec-releases`, PR titles, optional portal footer with package
version.

---

## 5. Where the “current version” is recorded

### In each `*-specification` repo (publisher)

| Object | Role |
|--------|------|
| Version file / release-please | Next package semver |
| GitHub Release tag + asset | Immutable publish |
| OpenAPI under `v{N}/schemas/` | Editable SSOT |

### In developer portal / CLI (consumers)

| File | Role |
|------|------|
| `pins.json` | Accepted version per `packageId` |
| Vendored specs / generated clients | Bytes derived from that pin |

### Do we still need `pins.json`?

**Yes.** Notify list = who to wake up. Pin = which version this repo accepted.
Without a pin you cannot answer “what is live on the portal?” or detect drift.

### 5.1 Package map (docs repo) — how sync finds the folder

The docs workflow does **not** infer paths from the GitHub repo name
(`domains.domain-lifecycle-specification` ≠ automatic
`openapi-specs/specs/domains-v3/`).

It uses a **checked-in map** keyed by `packageId` from the release payload.

**Proposed file:** `openapi-specs/package-map.json` (name can vary; concept is
required):

```json
{
  "domains/v3": {
    "sourceRepo": "gdcorp-platform/domains.domain-lifecycle-specification",
    "specsDir": "openapi-specs/specs/domains-v3",
    "registryKey": "domains-v3",
    "notes": "Domains Lifecycle Management API v3"
  }
}
```

| Field | Purpose |
|-------|---------|
| Map key / `packageId` | Same string the `*-specification` release puts in `client_payload.packageId` |
| `sourceRepo` | Expected publisher (optional validation) |
| `specsDir` | Folder to replace with the release tarball |
| `registryKey` | Key already used in `openapi-specs/registry.ts` for generate |

**How the three files work together:**

| File | Role |
|------|------|
| `package-map.json` | **Routing** — which `packageId` updates which folder / registry entry |
| `pins.json` | **Version** — which release that package currently accepted |
| `registry.ts` | **Generate wiring** — already exists; paths/output for fumadocs |

Sync algorithm:

1. Read `packageId` from dispatch.
2. Lookup `package-map.json[packageId]` → fail if missing.
3. Download artifact; replace `specsDir` only.
4. Set `pins.json[packageId]` to the new version/sha.
5. Run `bundle-specs` + `generate` (uses `registry.ts` as today).

Adding a new API to the portal = add a `registry.ts` entry **and** a
`package-map.json` row (and teach the publisher to send that `packageId`).
The §6.4 workflow `case` statement is a sketch of this lookup; production
should load `package-map.json` instead of hardcoding the case.

### 5.2 `pins.json` schema

**Location (docs):** `openapi-specs/pins.json`  
**Location (CLI):** e.g. `rust/schemas/api/pins.json` (same schema; different repo)

Top level is an object keyed by **`packageId`** (must match the release
`client_payload.packageId` and a key in `package-map.json` on docs).

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://godaddy.example/schemas/api-spec-pins.json",
  "title": "API spec consumer pins",
  "type": "object",
  "additionalProperties": { "$ref": "#/$defs/pinEntry" },
  "propertyNames": {
    "type": "string",
    "pattern": "^[a-z0-9][a-z0-9._-]*/v[0-9]+$"
  },
  "$defs": {
    "pinEntry": {
      "type": "object",
      "additionalProperties": false,
      "required": [
        "sourceRepo",
        "version",
        "tag",
        "sha256",
        "syncedAt"
      ],
      "properties": {
        "sourceRepo": {
          "type": "string",
          "description": "GitHub owner/name of the publishing *-specification repo",
          "pattern": "^[\\w.-]+/[\\w.-]+$"
        },
        "version": {
          "type": "string",
          "description": "Package semver (or date-version) of the accepted release",
          "examples": ["1.4.2", "2026-08-28"]
        },
        "tag": {
          "type": "string",
          "description": "Exact GitHub Release tag downloaded",
          "examples": ["v1.4.2", "domains-v3@1.4.2"]
        },
        "sha256": {
          "type": "string",
          "description": "Hex sha256 of the release asset that was applied",
          "pattern": "^[a-f0-9]{64}$"
        },
        "syncedAt": {
          "type": "string",
          "format": "date-time",
          "description": "When this consumer last wrote this pin (UTC)"
        },
        "assetUrl": {
          "type": "string",
          "format": "uri",
          "description": "Optional. Release asset URL used for the sync (audit/debug)"
        },
        "package": {
          "type": "string",
          "description": "Optional. npm/GitHub Packages name if published that way",
          "examples": ["@godaddy/api-spec-domains-v3"]
        }
      }
    }
  }
}
```

**Full example (docs after Domains sync):**

```json
{
  "domains/v3": {
    "sourceRepo": "gdcorp-platform/domains.domain-lifecycle-specification",
    "version": "1.4.2",
    "tag": "v1.4.2",
    "sha256": "a3f1c9e8b2d0471f6e5a90c4b8d3e7f0123456789abcdef0123456789abcdef0",
    "assetUrl": "https://github.com/gdcorp-platform/domains.domain-lifecycle-specification/releases/download/v1.4.2/domains-v3-1.4.2.tgz",
    "syncedAt": "2026-09-02T12:00:00Z"
  },
  "commerce.businesses/v1": {
    "sourceRepo": "gdcorp-platform/commerce.businesses-specification",
    "version": "2.1.0",
    "tag": "v2.1.0",
    "sha256": "b4e2d0f9c3a1582e7f6b01d5c9e4f80123456789abcdef0123456789abcdef01",
    "syncedAt": "2026-08-20T09:15:00Z"
  }
}
```

**Field rules**

| Field | Required | Rules |
|-------|----------|--------|
| Key (`packageId`) | Yes | Same as notify payload + `package-map.json` key |
| `sourceRepo` | Yes | Must match the publisher; sync may refuse mismatch vs map |
| `version` | Yes | Human/semver display; used in PR titles and drift compare |
| `tag` | Yes | Exact release tag passed to `gh release download` |
| `sha256` | Yes | Verify download before replacing files; 64-char lowercase hex |
| `syncedAt` | Yes | ISO-8601 UTC when the consumer applied the pin |
| `assetUrl` | No | Useful for audits; can be reconstructed from repo+tag |
| `package` | No | Only if also publishing to a package registry |

**Invariants**

1. Docs: every key in `pins.json` **must** exist in `package-map.json` (and the reverse for APIs that are sync-managed).
2. Sync updates **one key** per run; never rewrite unrelated pins.
3. Drift job: for each pin, compare `version`/`tag` to latest release on `sourceRepo`; open PR or alert if behind.
4. CLI may omit keys the CLI does not consume; docs and CLI pins need not match (intentional lag is OK and visible).

**What writes `pins.json`:** only the consumer sync workflow (bot PR). Humans do not hand-edit pins without also changing vendored/generated artifacts.

---

## 6. Detailed developer-portal sync

### 6.1 Trigger payload (example)

```json
{
  "event_type": "api-spec-release",
  "client_payload": {
    "packageId": "domains/v3",
    "packageVersion": "1.4.2",
    "tag": "v1.4.2",
    "assetUrl": "https://github.com/gdcorp-platform/domains.domain-lifecycle-specification/releases/download/v1.4.2/domains-v3-1.4.2.tgz",
    "sha256": "abcdef…",
    "sourceRepo": "gdcorp-platform/domains.domain-lifecycle-specification"
  }
}
```

### 6.2 Job behavior

1. Authenticate (App / machine user).
2. Download asset; verify checksum.
3. Map `packageId` → specs dir + registry key.
4. Replace that specs tree only (same OpenAPI file(s) from the release).
5. Rewrite pin; run generate; open/update PR.
6. Label `api-spec-sync`, semver class, product.

**Not smart format detection:** no Swagger↔OpenAPI conversion. Modern
specification repos ship OpenAPI 3.x; the job copies that artifact.

### 6.3 Branch protection

`openapi-specs/specs/**` bot-writable only.

### 6.4 Docs-repo workflow (update to a newer spec version)

Illustrative workflow in
`developer-ecosystem-documentation/.github/workflows/sync-api-spec.yml`.
Triggered when Domains (or any mapped API) publishes and dispatches
`api-spec-release`.

```yaml
# .github/workflows/sync-api-spec.yml
name: Sync API spec from release

on:
  repository_dispatch:
    types: [api-spec-release]
  workflow_dispatch:
    inputs:
      packageId:
        description: "e.g. domains/v3"
        required: true
      packageVersion:
        required: true
      tag:
        required: true
      assetUrl:
        required: true
      sha256:
        required: true
      sourceRepo:
        required: true

permissions:
  contents: write
  pull-requests: write

jobs:
  sync:
    runs-on: ubuntu-latest
    steps:
      - name: Read release payload
        id: meta
        run: |
          if [ "${{ github.event_name }}" = "repository_dispatch" ]; then
            echo "packageId=${{ github.event.client_payload.packageId }}" >> "$GITHUB_OUTPUT"
            echo "packageVersion=${{ github.event.client_payload.packageVersion }}" >> "$GITHUB_OUTPUT"
            echo "tag=${{ github.event.client_payload.tag }}" >> "$GITHUB_OUTPUT"
            echo "assetUrl=${{ github.event.client_payload.assetUrl }}" >> "$GITHUB_OUTPUT"
            echo "sha256=${{ github.event.client_payload.sha256 }}" >> "$GITHUB_OUTPUT"
            echo "sourceRepo=${{ github.event.client_payload.sourceRepo }}" >> "$GITHUB_OUTPUT"
          else
            echo "packageId=${{ inputs.packageId }}" >> "$GITHUB_OUTPUT"
            echo "packageVersion=${{ inputs.packageVersion }}" >> "$GITHUB_OUTPUT"
            echo "tag=${{ inputs.tag }}" >> "$GITHUB_OUTPUT"
            echo "assetUrl=${{ inputs.assetUrl }}" >> "$GITHUB_OUTPUT"
            echo "sha256=${{ inputs.sha256 }}" >> "$GITHUB_OUTPUT"
            echo "sourceRepo=${{ inputs.sourceRepo }}" >> "$GITHUB_OUTPUT"
          fi

      - uses: actions/checkout@v4

      - name: Map packageId → portal paths
        id: map
        run: |
          # Production: read openapi-specs/package-map.json (see §5.1).
          # Sketch equivalent:
          node -e '
            const id = process.env.PACKAGE_ID;
            const map = require("./openapi-specs/package-map.json");
            const e = map[id];
            if (!e) { console.error("Unknown packageId:", id); process.exit(1); }
            console.log(`specsDir=${e.specsDir}`);
            console.log(`registryKey=${e.registryKey}`);
          ' >> "$GITHUB_OUTPUT"
        env:
          PACKAGE_ID: ${{ steps.meta.outputs.packageId }}

      - name: Download and verify artifact
        run: |
          mkdir -p /tmp/spec-in
          curl -fsSL "${{ steps.meta.outputs.assetUrl }}" -o /tmp/spec.tgz
          echo "${{ steps.meta.outputs.sha256 }}  /tmp/spec.tgz" | sha256sum -c -
          tar -xzf /tmp/spec.tgz -C /tmp/spec-in

      - name: Replace vendored OpenAPI for this package only
        run: |
          SPECS_DIR="${{ steps.map.outputs.specsDir }}"
          rm -rf "${SPECS_DIR}"
          mkdir -p "${SPECS_DIR}"
          # Layout depends on tarball; adjust to match release packaging
          cp -R /tmp/spec-in/. "${SPECS_DIR}/"

      - name: Update pins.json
        run: |
          # Pseudocode: set pins["domains/v3"] = { version, tag, sha256, sourceRepo, syncedAt }
          node scripts/update-spec-pin.mjs \
            --packageId "${{ steps.meta.outputs.packageId }}" \
            --version "${{ steps.meta.outputs.packageVersion }}" \
            --tag "${{ steps.meta.outputs.tag }}" \
            --sha256 "${{ steps.meta.outputs.sha256 }}" \
            --sourceRepo "${{ steps.meta.outputs.sourceRepo }}"

      - uses: actions/setup-node@v4
        with:
          node-version: "20"
          cache: npm

      - name: Bundle + generate reference MDX
        run: |
          npm ci
          npm run bundle-specs
          npm run generate

      - name: Open PR
        uses: peter-evans/create-pull-request@v6
        with:
          token: ${{ secrets.GITHUB_TOKEN }}  # or GitHub App token
          branch: "chore/sync-${{ steps.map.outputs.registryKey }}-${{ steps.meta.outputs.packageVersion }}"
          title: "chore(specs): sync ${{ steps.meta.outputs.packageId }}@${{ steps.meta.outputs.packageVersion }}"
          body: |
            Automated sync from `${{ steps.meta.outputs.sourceRepo }}`.

            - Package: `${{ steps.meta.outputs.packageId }}`
            - Version: `${{ steps.meta.outputs.packageVersion }}`
            - Tag: `${{ steps.meta.outputs.tag }}`
            - SHA256: `${{ steps.meta.outputs.sha256 }}`

            Updates vendored OpenAPI, `pins.json`, bundled JSON, and generated
            reference MDX for this API only.
          labels: |
            api-spec-sync
            ${{ steps.map.outputs.registryKey }}
```

**What the PR typically contains** (Domains `1.4.1` → `1.4.2`):

| Path | Change |
|------|--------|
| `openapi-specs/specs/domains-v3/**` | Replaced with OpenAPI from the release tarball |
| `openapi-specs/pins.json` | `"domains/v3".version` → `1.4.2` (+ tag, sha256, sourceRepo) |
| `openapi-specs/bundled/domains-v3.json` | Regenerated by `bundle-specs` |
| `content/docs/references/rest/domains/v3/**` | Regenerated by `generate` |

Other APIs’ pins and folders are untouched. After merge, the normal docs deploy
publishes [Domains v3 reference](https://developer.godaddy.com/en/docs/references/rest/domains/v3).

**Manual retry:** `workflow_dispatch` with the same fields if a dispatch was
missed. **Weekly drift job** (optional second workflow) compares `pins.json` to
latest GitHub Releases and re-runs the same sync path.

---

## 7. Consumer list (hardcoded)

| Repo | Role |
|------|------|
| `gdcorp-commerce/developer-ecosystem-documentation` | Vendor + generate docs |
| `godaddy/cli` | Pin + codegen |
| *(optional)* one service | Pin + contract tests |

Keep this list in the **reusable workflow** used by all `*-specification`
repos so you edit it once.

---

## 8. Implementation steps (phased checklist)

### Phase 0 — Agree contracts (1–2 weeks)

- [ ] Confirm SSOT remains `*-specification` repos (no monorepo migration).
- [ ] Inventory portal keys: each maps to **one** upstream (`*-specification` or legacy `api-spec`), never both for the same API.
- [ ] Document what `api-spec` still reviews (exposure Private→Published) vs modern SSOT.
- [ ] Confirm tag/artifact scheme per repo.
- [ ] Confirm pin schema + `packageId` mapping (`domains/v3` → portal paths).
- [ ] Hardcoded consumers: docs + CLI (+ optional).
- [ ] Pilot: `domains.domain-lifecycle-specification` only.

### Phase 1 — Publisher workflow on Domains (1–3 weeks)

- [ ] Spectral + oasdiff on Domains specification CI.
- [ ] Release workflow: bundle OpenAPI → GitHub Release + checksum.
- [ ] Notify step: hardcoded docs + CLI dispatch.
- [ ] Extract reusable workflow; land caller in
      `api-specification-template` for future repos.
- [ ] Backport reusable caller to Domains (and later other live APIs).

**Done when:** Domains merge → release asset → dispatch fired.

### Phase 2 — Portal sync (2–4 weeks)

- [ ] `pins.json` + `package-map.json` for `domains/v3` (see §5.1).
- [ ] `sync-api-spec.yml` on docs repo (lookup via package-map).
- [ ] Branch protection on vendored specs.
- [ ] Auto-merge patch/minor.
- [ ] Pilot end-to-end →
      [portal Domains v3](https://developer.godaddy.com/en/docs/references/rest/domains/v3) updates.

### Phase 3 — CLI sync (1–3 weeks)

- [ ] Pins + regen-check for Domains.
- [ ] Same `repository_dispatch` handler.
- [ ] Auto-merge policy as desired.

### Phase 4 — Roll out to more `*-specification` repos

- [ ] Enable reusable release+notify on next APIs (businesses, channels, …).
- [ ] **Existing repos:** open PRs adding the thin caller workflow (template
      changes do not auto-propagate — backport required).
- [ ] **Scripted backport:** build `backport-release-caller.sh` (or similar) +
      `targets.csv` from `package-map.json`; open batch PRs (see §1.2).
- [ ] Prioritize repos already on the portal registry; defer unused internal specs.
- [ ] Extend docs/CLI mapping tables for each new `packageId`.
- [ ] Weekly drift job.
- [ ] Keep creating new APIs from `api-specification-template` (with workflow
      included). Aggregator submodule add remains manual for discovery.

### Phase 5 — Implementation verification (later)

- [ ] Service CI against pinned artifact (Schemathesis, etc.).

---

## 9. Sequence diagram

```text
Developer     domains.*-specification    GitHub Release     Docs repo           CLI repo
   │                    │                      │                │                   │
   │ PR OpenAPI         │                      │                │                   │
   │───────────────────►│                      │                │                   │
   │                    │ merge + release      │                │                   │
   │                    │─────────────────────►│                │                   │
   │                    │ notify (hardcoded)   │                │                   │
   │                    │──────────────────────────────────────►│                   │
   │                    │ notify               │                │                   │
   │                    │──────────────────────────────────────────────────────────►│
   │                    │                      │  pull asset    │                   │
   │                    │                      │◄───────────────│                   │
   │                    │                      │                │ pull asset        │
   │                    │                      │◄───────────────────────────────────│
   │                    │                      │  pin+gen PR    │  pin+codegen PR   │
   │                    │                      │     merge      │     merge         │
```

---

## 10. Worked example: “Is the portal in sync?”

1. Latest Domains release:
   `gh release view --repo gdcorp-platform/domains.domain-lifecycle-specification`
2. Portal pin: `openapi-specs/pins.json` → `domains/v3.version`
3. Match → in sync; catalog newer than pin → open sync PR / failed workflow.

---

## 11. FAQ

**Q: Since we keep individual repos, does each need a workflow that updates
consumers and the portal?**  
**Yes for publish + notify** — including **existing** `*-specification` repos
(not only new ones). Template updates do not rewrite old repos; add a thin
caller that invokes the reusable workflow (see §1.1). Roll out in waves
(Domains first → portal-backed APIs → rest). Docs and CLI each need **one**
sync workflow that handles any `packageId`.

**Q: Do we still consolidate into `api-spec`?**  
**No** for this design. Keep federated specification repos. `api-spec` stays
review/legacy.

**Q: What exactly is “reviewed” in `api-spec`?**  
Architecture / API-design review by exposure (Private → Published): automated
validation, integrator review, and Architecture Review (`@API Designers`) per
the [api-spec README](https://github.com/gdcorp-platform/api-spec/blob/master/README.md).
It is a **compliance gate + legacy catalog**, not the Domains v3 authoring home.

**Q: Must portal/CLI copy from both `api-spec` and `*-specification`?**  
**No for the same API.** One version → one upstream → one pin. Domains v3 →
only `domains.domain-lifecycle-specification`. Older APIs that still live only
in `api-spec` may keep a separate pin until migrated or retired.

**Q: OpenAPI or Swagger?**  
Modern specification repos and the portal/CLI Domains v3 path use **OpenAPI
3.x**. Sync **replaces** the published OpenAPI artifact for that pin — no
format conversion. Same logical file copied to mapped consumer paths; portal
may apply local generate transforms afterward.

**Q: Is the PR “smart” about versions?**  
Deterministic from the release payload (`packageId`, version, assetUrl,
sha256) + consumer mapping table — not AI.

**Q: Does a Domains release update Shoppers docs?**  
No. Only the pin/path for `domains/v3` changes.

**Q: Still need `pins.json`?**  
Yes.

**Q: How are new API repos created today?**  
From
[`api-specification-template`](https://github.com/gdcorp-platform/api-specification-template)
(`is_template: true`), then manually added to
[`api-specification-aggregator`](https://github.com/gdcorp-platform/api-specification-aggregator).

---

## 12. Related documents

- [Architect one-pager](./API_SPEC_SSOT_ARCHITECT_ONEPAGER.md)
- [CLI work breakdown (Domains pilot)](./API_SPEC_SSOT_CLI_WORK_BREAKDOWN.md)
- [API Spec SSOT + Downstream Sync (design)](./API_SPEC_SSOT_DOWNSTREAM_SYNC.md)
- [API Spec Repo Analysis](./API_SPEC_REPO_ANALYSIS_SSOT.md)

---

## 13. One-page summary

1. Edit OpenAPI only in the owning `*-specification` repo.
2. That repo’s workflow publishes a versioned OpenAPI artifact.
3. Same workflow **notifies hardcoded** docs (+ CLI) via `repository_dispatch`.
4. Consumer workflows pull the artifact, update **that** pin/path, open a PR.
5. Merge pin PR → portal/CLI are on the new version.
6. Reuse one workflow via the specification template; do not build an
   `api-spec` monorepo for this.
