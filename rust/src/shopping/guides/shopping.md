# Shopping API

`gddy shopping` integrates with the Shopping API.

## Authentication

Every Shopping command requests these OAuth scopes together, so the first command
can take the customer through one consent flow for the complete lifecycle:

```text
shopping.catalog:read
shopping.checkout:execute
shopping.order:read
```

You can authenticate before running a workflow:

```bash
gddy auth login \
  --scope shopping.catalog:read \
  --scope shopping.checkout:execute \
  --scope shopping.order:read
```

Shopping requests use the selected environment's standard API front door. Use OAuth
for Shopping commands.

## Workflow

Use `--body` for a small inline JSON request or `--file` for a reusable JSON document.
`--file` takes precedence over `--body`. The Shopping API uses nested checkout objects, so
checkout create, update, and complete requests remain JSON documents instead of a long list
of CLI flags. Use `gddy shopping <command> --help` for command-specific requirements; the
examples below show the request fields required for common checkout operations.

```bash
gddy --env test shopping catalog search --body '{}' --limit 3
gddy --env test shopping catalog lookup --body '{"ids":["nes-wsb-vnext-tier1"]}'
gddy --env test shopping catalog get --body '{"id":"nes-wsb-vnext-tier1"}'
gddy --env test shopping checkout create --body '{"context":{"currency":"USD"},"line_items":[{"item":{"id":"nes-wsb-vnext-tier1"},"quantity":1}]}'
gddy --env test shopping checkout update <checkout-id> --file update-checkout.json
```

Use a variant ID selected from `catalog search` as `line_items[].item.id`; product IDs are
for catalog lookup. For a full checkout update, start with the open checkout returned by
`shopping checkout get <checkout-id>`, edit the complete desired state, and send it with
`--file`. `update` replaces the checkout with the supplied document, so omitted fields may
be removed. An empty `line_items` array deliberately clears the cart.

### Catalog search and pagination

`catalog search` displays products as numbered sections. Each section shows its product
ID, then the purchasable variants and their prices. Use the **product ID** with
`catalog get` or `catalog lookup`; use a **variant ID** in `checkout create` line items.

`--limit` controls the number of returned **products**, not variants. A returned product
can contain multiple purchasable variants. To receive the original Shopping API response as
valid JSON—including product metadata, variants, messages, and pagination—use `--output json`:

```bash
gddy --env test --output json shopping catalog search --body '{}' --limit 3
```

Shopping API cursor pagination belongs in the request body's `pagination` object. Preserve all
original search criteria, retain the original `pagination.limit`, and replace only the
opaque `pagination.cursor` with the cursor from the preceding response:

```bash
gddy --env test shopping catalog search \
  --body '{"pagination":{"limit":3,"cursor":"<cursor from previous response>"}}'
```

Checkout updates use full-replacement `PUT` requests. PATCH is intentionally not
exposed by this CLI yet.

## Completing a checkout

`checkout complete` places a real order. Its Shopping API request must include a selected
saved payment instrument. You can supply a non-empty `idempotency_key`, or omit it to let
gddy generate one and return it in the completion result. Use the checkout's
`payment.instruments` list to select the saved instrument: mark exactly one entry with
`"selected": true`. For an uncertain completion, do not retry automatically; reuse the
effective idempotency key only for the same intended purchase after confirming its outcome.

```json
{
  "payment": {
    "instruments": [
      {
        "id": "<saved-payment-instrument-id>",
        "selected": true
      }
    ]
  },
  "idempotency_key": "<optional-stable-key>"
}
```

Omit `idempotency_key` to let gddy generate and return one. Include a stable key when you
need to control a later explicit retry.

```bash
gddy --env test shopping checkout complete <checkout-id> \
  --file complete-checkout.json
```

Completion returns immediately after the purchase attempt, including the effective
`idempotency_key` and order ID when the Shopping API provides one. New orders normally become
available 3–10 seconds after completion. Retrieve the order separately, optionally polling for
up to 15 seconds by default:

```bash
gddy --env test shopping order get <order-id> --wait --wait-timeout 15
```

After completion, use `shopping order get` with the returned order ID. `shopping checkout get`
is for open checkout sessions only.
