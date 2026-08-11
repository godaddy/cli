---
summary: Browse the GoDaddy API catalog, inspect operations, and make calls
---

# Exploring the GoDaddy API with `gddy api`

`gddy api` lets you browse GoDaddy's REST API catalog, inspect the details of any operation (parameters, request/response schemas, required scopes), and make an authenticated call — all without leaving the CLI or looking anything up in separate API docs.

The typical flow is: find an operation, inspect it, then call it.

## Finding an operation

If you don't already know the operation you want, start broad and narrow down:

```
gddy api domain list
```

lists every API domain in the catalog (a domain is a related group of endpoints, e.g. `domains`, `commerce`, `shipping`) along with how many operations each one has. Pick a domain and list its operations:

```
gddy api operation list --domain domains
```

which shows every operation's ID, HTTP method, path, and summary.

If you already have a rough idea what you're looking for — a keyword, a resource name, part of a path — skip straight to a full-text search across every domain at once:

```
gddy api search "dns record"
```

Search matches against operation IDs, paths, summaries, and descriptions, and returns the same kind of result rows `operation list` does.

## Inspecting an operation

Once you have an operation in mind, get its full detail with:

```
gddy api operation get <operationId>
```

You can pass either the operation's ID (e.g. `createOrder`) or a path or path fragment (e.g. `/v1/commerce/orders` or `/orders`). The output shows the HTTP method, path, summary, every parameter (including the request body, if any, shown as a parameter named `body`), every possible response, and the OAuth scopes the operation requires.

If your query matches more than one operation — a path shared by several HTTP methods, or a fuzzy search hitting several unrelated endpoints — `get` reports an error listing the candidates so you can narrow down, either by being more specific or by adding `--method`:

```
gddy api operation get /businesses --method POST
```

### Reading the parameter and response tables

Each parameter and response row shows a **Type** (e.g. `string`, `object`, `array<string>`) and, when there's more to it than that single word, a **Schema ID**. A schema ID is something you can pass to `gddy api schema get` (see below) to see the full nested structure — every property, its type, and whether it's required. A row with no schema ID (most simple string/number/boolean parameters) has nothing further to look up; its type is the whole story.

When an operation has a lot of parameters or responses, `operation get` only shows a preview (the table's footer tells you how many were shown out of the total). Follow the "Next steps" it prints — `gddy api parameter list` or `gddy api response list` for that operation — to see the rest.

## Parameters and responses in full

To list every parameter or response on an operation directly, rather than through the summary embedded in `operation get`:

```
gddy api parameter list --operation <operationId>
gddy api response list --operation <operationId>
```

Both support the standard `--limit`/`--offset` flags if there are more rows than fit on one page. To see one parameter or response's full detail on its own:

```
gddy api parameter get <name> --operation <operationId>
gddy api response get <status> --operation <operationId>
```

A parameter named `body` always refers to the operation's request body, if it has one.

## Schemas

Schemas — the shape of a request body, a parameter's value, or a response — can be nested and detailed, so they're kept out of the tables above except for a short preview. To see one in full:

```
gddy api schema get <id>
```

An `id` is either a shared component name shown as-is in a Schema ID column (e.g. `Business`), or a longer, dotted id scoped to one operation (e.g. `createOrder.responses.200.schema`) for a schema that isn't a reusable named component. Either way, copy the id straight from wherever you saw it — a Schema ID column, or a parameter/response's own detail view — you never need to construct one by hand. `schema get` always shows the complete structure; it's the last stop in this drill-down chain.

## Making a call

Once you know what an operation needs, call it:

```
gddy api call <path> --method <method>
```

`<path>` is the operation's actual request path (with real values, not the templated `{placeholders}` shown by `operation get` — e.g. `/v1/domains/example.com`, not `/v1/domains/{domain}`). Required OAuth scopes are resolved automatically from the catalog, merged with any you add explicitly via `--scope`.

Supply a request body one of three ways:

```
gddy api call <path> --method POST --body '{"name": "example"}'
gddy api call <path> --method POST --field name=example --field type=A
gddy api call <path> --method POST --file body.json
```

`--field` can be combined with `--body`/`--file` to override or add individual fields on top. Add extra headers with `--header 'Key: Value'` (repeatable), and see response headers alongside the body with `--include`.

Before running anything that changes data, preview it with the global `--dry-run` flag — it short-circuits before sending the request for any method that mutates state (a plain `GET`/`HEAD` still runs for real under `--dry-run`, since there's nothing unsafe to preview there).

## GraphQL operations

Some domains are backed by GraphQL rather than plain REST. In the catalog they still show up as one wrapper operation (`postTaxGraphql`, `postCatalogGraphql`) that accepts a generic `{query, variables}` body — but every GraphQL query and mutation that wrapper proxies to is also individually addressable.

GraphQL operations have their own dedicated set of commands. Run this to show a GraphQL operation's details:

```
gddy api graphql get <id>
```

A GraphQL operation contains:

- **Call Requirements** — what the underlying wrapper endpoint needs to route and authenticate the request (e.g. path parameters and HTTP headers). These are transport plumbing, not GraphQL semantics.
- **Arguments** — the GraphQL operation's own inputs.

Supply both of these with `--arg name=value` (repeatable).

```
gddy api graphql call postTaxGraphql::query::classification --arg storeId=<uuid> --arg x-store-id=<uuid> --arg id=<id>
```

The CLI builds the GraphQL query text and `variables` object for you, sends it to the HTTP wrapper, and reports the response or errors.

By default the response selects just `{ __typename }` for an object-shaped return type (or nothing at all for a plain scalar). To ask for specific fields, use `--select`, comma-separated and dot-nested for fields on a nested object:

```
gddy api graphql call postTaxGraphql::query::classification --arg id=<id> --select id,name,rate.percentage
```

### Finding out what's selectable

`gddy api graphql get <id>` lists the operation's **Return Type** and its **Return Fields** — one level deep. If a field's `Type` column is a plain scalar (`String`, `Int`, `ID`, ...) that's the whole story; if it's something else (e.g. `ClassificationListsConnection`), look that name up to see *its* fields:

```
gddy api graphql type get <TypeName>
```

which shows the same kind of field list (or, for an enum, the allowed values) for that type on its own, independent of any operation. Repeat with whatever type name shows up next — `edges` might be a `[SomethingEdge]`, look up `SomethingEdge`, find a `node` field, look up its type, and so on — until every segment of your `--select` path resolves to a real field.

A `mutation` operation is short-circuited under the global `--dry-run` flag the same way any other mutating call is; a `query` operation still runs for real, since there's nothing unsafe to preview.

### Getting the whole schema at once

Drilling one type at a time is the right flow for shaping a specific `--select`. If you want the actual GraphQL specification instead — the real SDL text, not a JSON reconstruction of it — ask for it directly:

```
gddy api graphql sdl get postTaxGraphql
```

This prints the `.graphql` schema source. Pass the domain's wrapper operation id (`postTaxGraphql`, `postCatalogGraphql`), not an individual query/mutation id.

Unlike every other command here, this one prints the SDL as plain text.

```
gddy api graphql sdl get postTaxGraphql > schema.graphql
```

If you'd rather have a structured, JSON-native dump instead of the raw SDL text — every operation and every type, with full field/arg detail and descriptions, already broken into our summary shape — that's available too:

```
gddy api operation get postTaxGraphql --output json
```

The result's `graphql` field has it all: `operations` and `types`. This is the same data `api graphql get`/`api graphql type get` read from, just all at once instead of one hop at a time — useful if you want to script against the JSON shape rather than parse GraphQL SDL yourself.

The wrapper operation itself (`postTaxGraphql`, `postCatalogGraphql`) keeps working exactly as before under the regular REST commands, if you'd rather hand-write the GraphQL query text yourself — `gddy api call postTaxGraphql --body '{"query": "...", "variables": {...}}'` is unaffected by any of the above.

## Quick reference

| Command | Purpose |
| --- | --- |
| `api domain list` | List every API domain |
| `api operation list --domain <domain>` | List operations in a domain |
| `api search <query>` | Full-text search across every domain |
| `api operation get <operationId>` | Full detail for one REST operation |
| `api parameter list/get --operation <id>` | A REST operation's parameters |
| `api response list/get --operation <id>` | A REST operation's responses |
| `api schema get <id>` | Full structure of a schema |
| `api call <path> --method <method>` | Make an authenticated REST request |
| `api graphql get <id>` | Full detail for one GraphQL operation |
| `api graphql call <id> --arg name=value` | Call a GraphQL operation |
| `api graphql type get <TypeName>` | Fields/values of a named GraphQL type |
| `api graphql sdl get <wrapperOperationId>` | The actual GraphQL schema (SDL) text, verbatim |
