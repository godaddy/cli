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

PATs are not currently accepted by the configured Shopping API endpoint. Use OAuth
until Shopping is exposed through the front door, where PAT exchange can occur.

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
saved payment instrument and a caller-owned, non-empty `idempotency_key`. Use the checkout's
`payment.instruments` list to select the saved instrument: mark exactly one entry with
`"selected": true`. Never create a new idempotency key when retrying an uncertain completion;
reuse the original key only for the same intended purchase.

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
  "idempotency_key": "<caller-generated-key>"
}
```

```bash
gddy --env test shopping checkout complete <checkout-id> \
  --file complete-checkout.json --wait-for-order
```

New orders normally become available 3–10 seconds after completion. `--wait-for-order` polls
the returned order ID for up to 15 seconds by default. You can also run:

```bash
gddy --env test shopping order get <order-id> --wait --timeout 15
```

After completion, use `shopping order get` with the returned order ID. `shopping checkout get`
is for open checkout sessions only.
