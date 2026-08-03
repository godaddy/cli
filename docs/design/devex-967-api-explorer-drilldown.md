# DEVEX-967: API explorer — command hyperlinks instead of truncation-to-file

Status: draft
Ticket: DEVEX-967
Related:
  - DEVEX-702 (reverted the temp-file prototype this design replaces)
  - DEVEX-716 (webhook truncation still uses temp files)
  - DEVEX-717 (actions catalog needs the same shape)

## Problem

During DEVEX-702 we prototyped truncating large `api describe` output (schemas, GraphQL operation lists) by writing the full content to a temp file and pointing to it from the response (`schema_detail`/`operationsDetail`). That was reverted before merge: arbitrary files on disk, no navigational structure, and test-isolation hazards from shared temp-file naming. DEVEX-702 shipped with **no truncation at all** — `rust/src/api_explorer/mod.rs` returns schemas and GraphQL operation lists in full, however large, as a deliberate placeholder.

The same file-writing shape already exists elsewhere and has the same problems:
- `rust/src/webhook/mod.rs` (`WebhookEventsOutput`, `write_full_output()`) writes truncated-over events to `$TMPDIR/godaddy-cli-<uuid>/webhook-events.json` — tracked as DEVEX-716.
- `rust/src/actions_catalog/mod.rs` needs equivalent `total/shown/truncated` + payload protection — tracked as DEVEX-717.

We want a durable replacement: instead of "here's a file with the rest," truncated output tells you the command to run for the rest. `cli-engine` already has this primitive — `NextAction`/`next_actions` (`cli-engine` `output/envelope.rs`, rendered as a "Next steps:" footer in human output and a `next_actions` array in JSON) — and `rust/src/next_action.rs` wraps it. `api_explorer` already uses it for fuzzy-match disambiguation (`mod.rs` `multi_match_result`). This design turns that into a standing convention for truncation, and builds out the API explorer's entity/command surface so every truncatable thing has somewhere to point.

## Goals

- Replace the temp-file truncation pattern everywhere it exists or was planned (`api_explorer`, and as a model for DEVEX-716/717) with a `next_actions`-based convention.
- Give every API entity below the operation level (parameter, request/response schema, response) a stable identifier and a dedicated command, so `describe` output isn't the only way to reach them.
- Make pagination command-driven with sane defaults, not purely opt-in via `--limit`/`--offset`.
- Define the rule for what a truncated nested section points to, so the convention is unambiguous as we add entity types later.

## Non-goals

- **Nested table human output.** `cli-engine`'s human renderer supports one flat table (arrays) or one flat property bag (objects) — nested objects/arrays currently serialize inline as JSON strings (`output/human.rs`: `render_data_body`, `format_value`). Making the human renderer support child tables is its own design problem (layout, terminal width, recursion depth) and ships as a separate ticket. JSON output already carries full structure; this design's job is to make sure truncation/drill-down works correctly regardless of how human output eventually renders it.
- Re-litigating whether truncation should be eager (always summarized) vs. lazy (full unless too big) — out of scope here; the envelope and command surface work the same either way.

## Design

### 1. Entities and identifiers

Not every entity needs a globally unique ID. Only schemas do (see below); parameters and responses are addressed by a short name/status code plus a scoping flag on the command:

| Entity | Example ID | Notes |
|---|---|---|
| Domain | `partners` | already stable today |
| Operation | `getUser` (operationId) or path fragment | already stable today |
| Parameter | `limit` | not globally unique — scoped by the `--operation` flag on the command, not baked into the id itself. The request body folds in here too, as `in: "body"` (see below). |
| Response | `200` | same scoping rule: id is just the status code, `--operation` supplies the rest |
| Operation | `getUser` (operationId) | `--operation <id>` alone is enough — no `--domain` needed. `find_endpoint_exact` (`rust/src/api_explorer/mod.rs:275-294`) already resolves operations catalog-wide today, matching only by `operationId`/path (ambiguity is handled via `--method`, never domain), so `describe`/`call`/`parameter`/`response` all share the same assumption: operationId is globally unique. Nothing enforces that at generation time — `generate-api-catalog` only dedupes domain names (`main.rs:227-229`) — so this is a pre-existing gap this design inherits rather than introduces, worth a separate follow-up if it's ever hit in practice. |
| Schema | `getUser.responses.200.schema`, or a `$ref` component name like `User` | the one entity where a nested/dotted id earns its keep — an inline schema has no name of its own and needs a path to identify it, while a referenced (`$ref`) schema already has a stable global name we should just reuse |

**Request body folds into parameters.** In the catalog's flattened shape (`Endpoint.request_body`: `{ required, contentType, schema }` — the catalog generator already collapses OpenAPI 3.x's per-content-type `requestBody.content` map into one, so we don't inherit that complexity), a request body is structurally just a parameter missing a `name`/`in`. Model it as a synthetic parameter entry with `name: "body"`, `in: "body"`, so `api parameter list --operation <id>` enumerates query/path/header/cookie params and the body together, and `api parameter get body --operation <id>` reaches it the same way as any other parameter. Today's raw `parameters: Vec<Value>` (`rust/src/api_explorer/mod.rs:91`) needs a real `ParameterSummary` type regardless, now that parameter is a first-class command — folding the body in costs nothing extra. One edge case: a literal query/path parameter named `body` would collide with the synthetic entry; worth a validation check at catalog-generation time, though not expected in practice.

**Schema is atomic**, not recursively addressable, and — unlike every other entity here — `api schema get <id>` never truncates. It's the terminal drill-down target for a schema: there's nowhere further to point, so it always returns the complete nested tree (properties-of-properties, array items, `oneOf`/`allOf`, `$ref` resolution — all inline, at every level), where `<id>` is whichever the schema already has: a `$ref` component name if it's shared, or a synthesized dotted path if it's inline and unnamed.

Truncation only happens on an *embedded preview* of a schema — the schema field shown inside a parameter's, response's, or `describe`'s output — which trims breadth within a level (e.g. "showing 20 of 32 properties") purely for display size, and always links to the untruncated `api schema get <id>` rather than paginating itself. This keeps the ID grammar to one dotted path per unnamed schema instead of minting an ID for every nested property, and avoids a combinatorial ID explosion for deeply nested types. It also means a schema's full tree is never itself broken into pages — if a real one turns out to be unusably large even in full, that's a case to revisit (see open questions), not something solved by adding pagination to `schema get`.

Note `generate-api-catalog` already had to resolve a `$defs` key-collision bug for same-file property refs (DEVEX-965, `rust/tools/generate-api-catalog/src/main.rs:2278`) — schema ID minting needs to account for the same collision surface (two inline schemas from different source files landing on the same dotted path).

### 2. New commands, and renaming `describe`/`endpoint` to `operation`

Mirroring the existing `api domain` group pattern, and treating "operation" as a first-class entity name consistently with the `--operation` scoping flag used elsewhere in this design:

- `api operation list --domain <domain>` — renamed from `api endpoint list --domain <domain>`. `Endpoint` (`rust/src/api_explorer/mod.rs:81-100`) is already one `method`+`path`+`operationId` row — structurally an OpenAPI operation, not a path grouping multiple methods — so "endpoint" was the wrong name for what this already is.
- `api operation get <id>` — renamed from `api describe <id>`. Stops inlining full parameter/response/schema payloads (the request body appears as just another row in the parameter summary, not a section of its own). Becomes an index: summarized rows for parameters and responses (name/status + one-line summary each), and `next_actions` pointing at the commands below for anything summarized or truncated.
- `api parameter list --operation <id>` — paginated list of an operation's parameters, including the request body folded in as `name: "body", in: "body"` (summary rows: name, `in`, required, truncated schema hint).
- `api parameter get <name> --operation <id>` — full parameter detail (query/path/header/cookie, or `body`), including its schema (which may itself be truncated → next action to `api schema get`).
- `api response list --operation <id>` — paginated list of an operation's responses (summary rows: status, truncated schema hint).
- `api response get <status> --operation <id>` — full response detail for one status code.
- `api schema get <id>` — full schema tree for any parameter/response/request-body schema. One command regardless of where the schema is attached, addressed by its `$ref` name or synthesized dotted path.

`api call <path>` is unaffected — it deliberately stays addressed by a literal, parameter-substituted URL path rather than an operation id. It already rejects a bare `operationId` (`mod.rs:1161-1173`) because the real HTTP request has to be built from that literal path, not a catalog lookup key; that's a different addressing concern from "which entity am I looking at."

`describe`/`endpoint list` are currently `Stage::Ga` (no `.with_feature_flag(...)` in `api_explorer/mod.rs` — see `docs/feature-flags.md`), so this rename is a breaking change for any existing script or agent muscle-memory built on the old names — see open questions.

### 3. Truncation envelope — one shared pattern, lives in `gddy`

A single reusable convention (a struct/helper in `rust/src`, not `cli-engine` — no other module outside `gddy` needs it yet) replaces both the reverted temp-file prototype and `webhook`'s existing one:

```rust
struct Summary<T> {
    items: Vec<T>,
    total: usize,
    shown: usize,
    truncated: bool,
}
```

No `full_output` / file path field. Instead, a fixed rule for what `next_actions` gets attached:

- **List truncated (1:many)** — e.g. an operation's parameters, an operation's responses, `api operation list` — the next action points at the **standalone list command** for that child type (`api parameter list --operation <id>`), not at "page 2 of the embedded copy." The standalone command has its own default page size and does its own truncation independently — the parent and child views are allowed to truncate at different sizes.
- **Single nested entity truncated (1:1 or many:1)** — e.g. a parameter's schema, a response's schema — the next action points at the **standalone get command** for that entity (`api schema get <id>`). Same rule: no pagination params on this next action, because there's nothing to paginate — it's "go see the whole thing," not "see more of the same list."
- The standalone view generates its **own** `next_actions` for its own truncation (e.g. `api parameter list` page 2) — truncation-driven navigation is always local to the command you're looking at, never inherited from the parent that linked you here.

This is the same rule whether the relationship is one-to-many (parameter list) or one-to-one (parameter → schema) — the distinguishing factor is just whether the next action carries pagination params or not.

Migration for existing offenders, once this lands:
- **DEVEX-716** (webhook): replace `WebhookEventsOutput`/`write_full_output` with `Summary<WebhookEvent>` + a `next_actions` entry pointing at a (new, if needed) paginated `webhook events list` invocation with explicit offset.
- **DEVEX-717** (actions catalog): same shape for `actions list`; `protectPayload` stays as its own concern (payload redaction, not truncation) and composes independently.

### 4. Command-driven pagination (cli-engine change)

Today, `--limit`/`--offset` are global flags (`cli-engine` `flags.rs`) wired into every command's `--help` regardless of whether that command does anything with them, and `apply_pagination` (`output/pipeline.rs`) only runs when `limit > 0 || offset > 0` — pure opt-in, no default page size, no way for a command to declare "I paginate, and my default page is 20."

Add a builder method on `CommandSpec`, alongside the existing `.with_feature_flag(...)`:

```rust
.with_pagination(PaginationConfig {
    default_limit: 20,
    max_limit: 100,
    ..Default::default()
})
```

Effects:
- `--limit`/`--offset` args are only registered (and thus only show up in `--help`) for commands that call `.with_pagination(...)`. Commands that don't opt in stop advertising flags they ignore.
- When a paginating command runs with neither flag passed, `default_limit` applies instead of the current "pagination disabled" sentinel — pagination becomes command-driven, not just user-driven.
- `max_limit` caps what a user can request with `--limit`, independent of the default.

This lives in `cli-engine`, which is a published crates.io dependency here (not a path dep) — the change needs a `cli-engine` release and a version bump in `rust/Cargo.toml` before any `gddy`-side command can use it. Sequence this first; everything else in this design can be built against the *interface* this exposes, but can't ship end-to-end until the bump lands.

### 5. Example flow

```
$ gddy api operation get getUser
{
  operationId: "getUser",
  parameters: { total: 12, shown: 5, truncated: true, items: [...] },
  responses:   { total: 3,  shown: 3, truncated: false, items: [...] },
  ...
  next_actions: [
    { command: "gddy api parameter list --operation getUser",
      description: "View all 12 parameters" }
  ]
}

$ gddy api parameter get limit --operation getUser
{
  name: "limit", in: "query", required: false,
  schema: { type: "integer", truncated: false },   # small enough to inline
  ...
}

$ gddy api response get 200 --operation getUser
{
  status: 200,
  schema: { type: "object", properties: { ... 20 of 34 ... }, truncated: true },
  next_actions: [
    { command: "gddy api schema get getUser.responses.200.schema",
      description: "View full response schema" }
  ]
}
```

## Sequencing

1. `cli-engine`: add `.with_pagination(...)` + default-limit support, publish, bump `rust/Cargo.toml`.
2. `gddy`: land the shared `Summary<T>` + next-actions-on-truncation convention.
3. `generate-api-catalog`: mint dotted-path/`$ref`-name IDs for schemas (parameters and responses need no minted id — a name/status code plus `--operation` is enough); account for the DEVEX-965 collision surface.
4. `api_explorer`: rename `describe`→`operation get` and `endpoint list`→`operation list`; add `parameter`/`response`/`schema` commands; rework `operation get` to summarize + link instead of inlining. Update every place that currently hardcodes `api describe <operationId>` as a `next_action` template or help string — at minimum `multi_match_result` (`mod.rs:301-330`) and `search_command` (`mod.rs:1004-1023`, `1046`) — plus the command's own help text (`mod.rs:869`, `873`).
5. Migrate `webhook` (DEVEX-716) and `actions_catalog` (DEVEX-717) onto the shared convention — coordinate with their current owners (jpearlman, mguerrero3) since both tickets are actively in progress.

## Open questions

- Exact dotted-path grammar for IDs that must survive catalog regeneration (stability across regen matters if anything caches an ID) — needs a pass once we're minting them for real specs, not just the shape described above.
- Whether `max_limit` should be a hard server-side error or a silently-clamped value when a user passes `--limit` above it.
- Nested table human rendering — tracked as a separate follow-up ticket (not filed yet).
- What to do if a schema's untruncated tree is itself too large to return usefully (e.g. hundreds of properties or deep `$ref` recursion) — not expected to be common, but `api schema get` has no further place to point if it happens, since it's the terminal command in this design.
- Whether renaming `describe`→`operation get` and `endpoint list`→`operation list` needs a deprecation period (keep the old names as hidden aliases that still work, maybe emitting a warning) or can just ship as a clean break — both are currently `Stage::Ga`, so anyone scripting against the old names today would break either way.
