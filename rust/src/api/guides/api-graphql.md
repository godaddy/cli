---
summary: Inspect and call GraphQL-backed APIs, choose the fields you get back, and read the schema
---

# Call a GraphQL API with `gddy api graphql`

Some GoDaddy API domains are backed by GraphQL rather than plain REST. `gddy api graphql` lets you inspect their operations, call them, choose the fields you get back, and read the schema. To find an operation, or to call a REST operation, go to `gddy guide api-explorer`.

## Concepts

The following terms and concepts are relevant to calling GraphQL-backed APIs:

**Wrapper operation**: the one REST operation (for example `postTaxGraphql` or `postCatalogGraphql`) through which a domain backed by GraphQL accepts a generic `{query, variables}` body. In the catalog, the domain shows up as this one operation.

**GraphQL operation**: a single GraphQL query or mutation that the wrapper operation proxies to. Each one is also individually addressable, with an ID, for example `postTaxGraphql::query::classification`.

**SDL**: GraphQL's schema definition language, the text form of a GraphQL schema.

## Inspect a GraphQL operation

GraphQL operations have their own dedicated set of commands. Run this to show a GraphQL operation's details:

```
gddy api graphql get <id>
```

A GraphQL operation contains:

- **Call Requirements**: what the wrapper operation needs to route and authenticate the request (for example path parameters and HTTP headers). They come from the REST endpoint that carries the query, not from GraphQL itself.
- **Arguments**: the GraphQL operation's own inputs.

## Call a GraphQL operation

Supply the call requirements and the arguments with `--arg name=value` (repeatable):

```
gddy api graphql call postTaxGraphql::query::classification --arg storeId=<uuid> --arg x-store-id=<uuid> --arg id=<id>
```

The CLI builds the GraphQL query text and `variables` object for you, sends it to the wrapper operation, and reports the response or errors.

By default the response selects just `{ __typename }` for an object-shaped return type (or nothing at all for a plain scalar). To ask for specific fields, use `--select`, comma-separated and dot-nested for fields on a nested object:

```
gddy api graphql call postTaxGraphql::query::classification --arg storeId=<uuid> --arg x-store-id=<uuid> --arg id=<id> --select id,name,rate.percentage
```

The global `--dry-run` flag short-circuits a `mutation` operation the same way it does any other mutating call. A `query` operation still runs for real, since there's nothing unsafe to preview.

## Find what's selectable

`gddy api graphql get <id>` lists the operation's **Return Type** and its **Return Fields**, one level deep. If a field's `Type` column is a plain scalar (`String`, `Int`, `ID`, ...), that's the whole story. If it's something else (for example `ClassificationListsConnection`), look that type up on its own, independent of any operation. This shows the same kind of field list, or the allowed values for an enum:

```
gddy api graphql type get <TypeName>
```

Repeat with whatever type name shows up next until every segment of your `--select` path resolves to a real field. For example, `edges` might be a `[SomethingEdge]`: look up `SomethingEdge`, find a `node` field, look up its type, and so on.

A GraphQL type name is only unique within its own domain's schema. Two domains can each declare an unrelated type sharing a name (for example `ReferenceValueFilter` on both `taxes` and `catalog-products`). When that happens, `api graphql type get <TypeName>` errors out listing every domain that defines it. Add `--domain <domain>` to pick one.

## Get the whole schema at once

Drilling one type at a time is the right flow for shaping a specific `--select`. If you want the actual GraphQL specification instead (the real SDL text, not a JSON reconstruction of it), ask for it directly:

```
gddy api graphql sdl get postTaxGraphql
```

This prints the `.graphql` schema source. Pass the domain's wrapper operation id (`postTaxGraphql`, `postCatalogGraphql`), not an individual query or mutation id.

Unlike every other command here, this one prints the SDL as plain text, so you can save it to a file:

```
gddy api graphql sdl get postTaxGraphql > schema.graphql
```

If you'd rather have a structured, JSON-native dump instead of the raw SDL text, that's available too. It includes every operation and every type, with full field and argument detail and descriptions:

```
gddy api operation get postTaxGraphql --output json
```

The result's `graphql` field has it all: `operations` and `types`. This is the same data `api graphql get` and `api graphql type get` read from, just all at once instead of one hop at a time. It's useful if you want to script against the JSON shape rather than parse GraphQL SDL yourself.

The wrapper operation itself (`postTaxGraphql`, `postCatalogGraphql`) keeps working exactly as before under the regular REST commands, if you'd rather hand-write the GraphQL query text yourself. `api call` takes a concrete path, not an operation ID. Grab the path from `fullPath` in the output of `api operation get postTaxGraphql`. The call then looks like `gddy api call /v2/commerce/stores/<storeId>/tax-subgraph --body '{"query": "...", "variables": {...}}'`. The GraphQL commands in this guide don't change it.

## Command reference

| Command | Purpose |
| --- | --- |
| `api graphql get <id>` | Full detail for one GraphQL operation |
| `api graphql call <id> --arg name=value` | Call a GraphQL operation |
| `api graphql type get <TypeName>` | Fields or values of a named GraphQL type |
| `api graphql sdl get <wrapperOperationId>` | The actual GraphQL schema (SDL) text, verbatim |

## See also

- `gddy guide api-explorer`: find operations and call REST operations.
- `gddy guide auth`: how authentication and credential storage work.
