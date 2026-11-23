---
summary: Browse the GoDaddy API catalog, inspect operations, and make calls
---

# Explore the GoDaddy API with `gddy api`

`gddy api` lets you browse GoDaddy's API catalog and inspect the details of operations, like data schemas and required scopes. You can also make authenticated calls to these APIs. You never need to leave the CLI or look anything up in separate API docs.

The typical flow is: find an operation, inspect it, then call it.

## Concepts

The following terms and concepts are relevant to browsing and calling the GoDaddy API:

**API domain**: a related group of endpoints, for example `domains`, `commerce` or `shipping`. It isn't a domain name.

**Operation**: one endpoint of the API, identified by an operation ID (for example `createOrder`), an HTTP method, and a path.

**Catalog**: the list of API domains and operations that `gddy api` browses.

**Schema**: the shape of a request body, a parameter's value, or a response. A schema ID is a name you can pass to `gddy api schema get` to see the full structure.

**Scope**: the OAuth permission an operation requires. `gddy api call` works out the required scopes for you.

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

You can pass either the operation's ID (for example `createOrder`), a path, or a path fragment (for example `/v1/commerce/orders` or `/orders`). The output shows the HTTP method, path, and summary. It also shows every parameter, every possible response, and the OAuth scopes the operation requires. A request body, if there is one, shows as a parameter named `body`.

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

Some API domains are backed by GraphQL rather than plain REST. Go to `gddy guide api-graphql` for how to inspect and call a GraphQL operation, choose the fields you get back, and read the schema.

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

## See also

- `gddy guide auth`: how authentication and credential storage work.
- `gddy guide api-graphql`: inspect and call GraphQL-backed APIs.
