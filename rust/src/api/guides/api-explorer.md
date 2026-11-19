---
summary: Browse the GoDaddy API catalog, inspect operations, and make calls
---

# Explore the GoDaddy API with `gddy api`

`gddy api` lets you browse GoDaddy's REST API catalog and inspect the details of any operation: parameters, request and response schemas, and required scopes. You can also make an authenticated call. You never need to leave the CLI or look anything up in separate API docs.

The typical flow is: find an operation, inspect it, then call it.

## Concepts

The following terms and concepts are relevant to browsing and calling the GoDaddy API:

**API domain**: a related group of endpoints, for example `domains`, `commerce` or `shipping`. It isn't a domain name.

**Operation**: one endpoint of the API, identified by an operation ID (for example `createOrder`), an HTTP method, and a path.

**Catalog**: the list of API domains and operations that `gddy api` browses. You don't need to look anything up outside the CLI.

**Schema**: the shape of a request body, a parameter's value, or a response. A schema ID is a name you can pass to `gddy api schema get` to see the full structure.

**Scope**: the OAuth permission an operation requires. `gddy api call` works out the required scopes for you.

**Wrapper operation**: the one REST operation (for example `postTaxGraphql`) through which a domain backed by GraphQL accepts queries. Every GraphQL query and mutation behind it's also addressable on its own.

**SDL**: GraphQL's schema definition language, the text form of a GraphQL schema.

## Find an operation

If you don't already know the operation you want, start broad and narrow down. List every API domain in the catalog, with the number of operations each one has:

```
gddy api domain list
```

Pick a domain and list its operations. The output shows every operation's ID, HTTP method, path, and summary:

```
gddy api operation list --domain domains
```

If you already know roughly what you want (a keyword, a resource name, part of a path), skip straight to a full-text search across every domain:

```
gddy api search "dns record"
```

Search matches against operation IDs, paths, summaries, and descriptions, and returns the same kind of result rows `operation list` does.

## Inspect an operation

After you have an operation in mind, get its full detail with:

```
gddy api operation get <operationId>
```

You can pass either the operation's ID (for example `createOrder`) or a path or path fragment (for example `/v1/commerce/orders` or `/orders`). The output shows the HTTP method, path, and summary. It also shows every parameter, every possible response, and the OAuth scopes the operation requires. A request body, if there is one, shows as a parameter named `body`.

If your query matches more than one operation, `get` reports an error listing the candidates. This happens with a path shared by several HTTP methods, or a fuzzy search hitting several unrelated endpoints. Be more specific, or add `--method`:

```
gddy api operation get /businesses --method POST
```

### Read the parameter and response tables

Each parameter and response row shows a **Type** (for example `string`, `object`, `array<string>`) and, when there's more to it than that single word, a **Schema ID**. A schema ID is something you can pass to `gddy api schema get` to see the full nested structure: every property, its type, and whether it's required. Go to the "Look up a schema" section later in this guide for details. A row with no schema ID (most simple string, number, and boolean parameters) has nothing further to look up. Its type is the whole story.

When an operation has a lot of parameters or responses, `operation get` only shows a preview (the table's footer tells you how many were shown out of the total). To see the rest, follow the "Next steps" it prints: `gddy api parameter list` or `gddy api response list` for that operation.

## List parameters and responses in full

To list every parameter or response on an operation directly, rather than through the summary embedded in `operation get`:

```
gddy api parameter list --operation <operationId>
gddy api response list --operation <operationId>
```

Both support the standard `--limit` and `--offset` flags if there are more rows than fit on one page. To see one parameter or response's full detail on its own:

```
gddy api parameter get <name> --operation <operationId>
gddy api response get <status> --operation <operationId>
```

A parameter named `body` always refers to the operation's request body, if it has one.

## Look up a schema

Schemas (the shape of a request body, a parameter's value, or a response) can be nested and detailed. The parameter and response tables only show a short preview of each one. To see one in full:

```
gddy api schema get <id>
```

An `id` is one of two things. It's either a shared component name shown as-is in a Schema ID column (for example `Business`), or a longer, dotted id scoped to one operation (for example `createOrder.responses.200.schema`). The dotted form is for a schema that isn't a reusable named component. Either way, copy the id straight from wherever you saw it: a Schema ID column, or a parameter or response's own detail view. You never need to construct one by hand. `schema get` always shows the complete structure. It's the last stop in this drill-down chain.

## Make a call

After you know what an operation needs, call it:

```
gddy api call <path> --method <method>
```

`<path>` is the operation's actual request path, with real values, not the templated `{placeholders}` shown by `operation get`. For example, use `/v1/domains/example.com`, not `/v1/domains/{domain}`. The CLI resolves the required OAuth scopes from the catalog automatically and merges them with any you add with `--scope`.

Supply a request body one of three ways:

```
gddy api call <path> --method POST --body '{"name": "example"}'
gddy api call <path> --method POST --field name=example --field type=A
gddy api call <path> --method POST --file body.json
```

`--field` can be combined with `--body` or `--file` to override or add individual fields on top. Add extra headers with `--header 'Key: Value'` (repeatable), and see response headers alongside the body with `--include`.

Before running anything that changes data, preview it with the global `--dry-run` flag. It short-circuits before sending the request for any method that mutates state. A plain `GET` or `HEAD` still runs for real under `--dry-run`, since there's nothing unsafe to preview there.

## Make a GraphQL call

Some domains are backed by GraphQL rather than plain REST. In the catalog they still show up as one wrapper operation (`postTaxGraphql`, `postCatalogGraphql`) that accepts a generic `{query, variables}` body. But every GraphQL query and mutation that wrapper proxies to is also individually addressable.

GraphQL operations have their own dedicated set of commands. Run this to show a GraphQL operation's details:

```
gddy api graphql get <id>
```

A GraphQL operation contains:

- **Call Requirements**: what the wrapper operation needs to route and authenticate the request (for example path parameters and HTTP headers). They come from the REST endpoint that carries the query, not from GraphQL itself.
- **Arguments**: the GraphQL operation's own inputs.

Supply both of these with `--arg name=value` (repeatable).

```
gddy api graphql call postTaxGraphql::query::classification --arg storeId=<uuid> --arg x-store-id=<uuid> --arg id=<id>
```

The CLI builds the GraphQL query text and `variables` object for you, sends it to the wrapper operation, and reports the response or errors.

By default the response selects just `{ __typename }` for an object-shaped return type (or nothing at all for a plain scalar). To ask for specific fields, use `--select`, comma-separated and dot-nested for fields on a nested object:

```
gddy api graphql call postTaxGraphql::query::classification --arg storeId=<uuid> --arg x-store-id=<uuid> --arg id=<id> --select id,name,rate.percentage
```

### Find what's selectable

`gddy api graphql get <id>` lists the operation's **Return Type** and its **Return Fields**, one level deep. If a field's `Type` column is a plain scalar (`String`, `Int`, `ID`, ...), that's the whole story. If it's something else (for example `ClassificationListsConnection`), look that type up on its own, independent of any operation. This shows the same kind of field list, or the allowed values for an enum:

```
gddy api graphql type get <TypeName>
```

Repeat with whatever type name shows up next until every segment of your `--select` path resolves to a real field. For example, `edges` might be a `[SomethingEdge]`: look up `SomethingEdge`, find a `node` field, look up its type, and so on.

A GraphQL type name is only unique within its own domain's schema. Two domains can each declare an unrelated type sharing a name (for example `ReferenceValueFilter` on both `taxes` and `catalog-products`). When that happens, `api graphql type get <TypeName>` errors out listing every domain that defines it. Add `--domain <domain>` to pick one.

The global `--dry-run` flag short-circuits a `mutation` operation the same way it does any other mutating call. A `query` operation still runs for real, since there's nothing unsafe to preview.

### Get the whole schema at once

Drilling one type at a time is the right flow for shaping a specific `--select`. If you want the actual GraphQL specification instead (the real SDL text, not a JSON reconstruction of it), ask for it directly:

```
gddy api graphql sdl get postTaxGraphql
```

This prints the `.graphql` schema source. Pass the domain's wrapper operation id (`postTaxGraphql`, `postCatalogGraphql`), not an individual query or mutation id.

Unlike every other command here, this one prints the SDL as plain text.

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
| `api graphql type get <TypeName>` | Fields or values of a named GraphQL type |
| `api graphql sdl get <wrapperOperationId>` | The actual GraphQL schema (SDL) text, verbatim |

## See also

- `gddy guide auth`: how authentication and credential storage work.
