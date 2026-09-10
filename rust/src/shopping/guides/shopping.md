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

## Environment

Commands below use the active environment. To run them against another configured
environment, add `--env <environment>` before `shopping`:

```bash
gddy --env <environment> shopping catalog search --body '{}'
```

## Request documents and output

Use `--body` for a small inline JSON request or `--file` for a reusable JSON document.
`--file` takes precedence over `--body`. The Shopping API uses nested checkout objects,
so checkout create, update, and complete requests remain JSON documents instead of a
long list of CLI flags.

JSON is the default output format. For successful non-dry-run requests, the full
Shopping API response is in the envelope's `data` field. Add `--output human` for a
concise terminal presentation, or `--output json` to make the default explicit. Use
`gddy shopping <command> --help` for command-specific requirements.

## Discover products

Search the catalog with an optional ISO 4217 presentment-currency preference:

```bash
gddy shopping catalog search --currency GBP --limit 3 --body '{}'
```

`--currency` writes `context.currency` into the request. You can instead place it in
the JSON document. Supplying both with different values fails. The API response price
currency is authoritative; a requested currency is a preference, not a guarantee. The
current Shopping service applies the preference to catalog search; catalog lookup and
product retrieval currently accept the context but can return their default USD prices.

`catalog search` displays products as numbered sections. Each section shows its product
ID, then purchasable variants and prices. `--limit` controls **products**, not variants.
Use a **product ID** with `catalog get` or `catalog lookup`; use a **variant ID** in
`checkout create` line items.

```bash
gddy shopping catalog lookup \
  --body '{"ids":["nes-wsb-vnext-tier1"]}' --currency GBP

gddy shopping catalog get \
  --body '{"id":"nes-wsb-vnext-tier1"}' --currency GBP
```

To receive the full catalog response as valid JSON:

```bash
gddy --output json shopping catalog search --body '{}' --limit 3
```

Shopping API cursor pagination belongs in the request body's `pagination` object.
Preserve all original search criteria—including `context.currency`—retain the original
`pagination.limit`, and replace only the opaque `pagination.cursor`:

```bash
gddy shopping catalog search \
  --body '{"context":{"currency":"GBP"},"pagination":{"limit":3,"cursor":"<cursor from previous response>"}}'
```

## Create a checkout ready to complete

Creating a checkout does not place an order. A create response includes the checkout ID,
priced line items, totals, and available payment instruments, so a separate `checkout get`
is not required before completion when the checkout is already ready. Include buyer, payment,
and other supported checkout information when creating a checkout that is ready to complete.

```bash
gddy shopping checkout create --body '{
  "context": {"currency": "USD"},
  "line_items": [
    {
      "item": {"id": "<available-variant-id>"},
      "quantity": 1
    }
  ],
  "buyer": {
    "first_name": "Jane",
    "last_name": "Doe",
    "email": "jane.doe@example.test",
    "phone_number": "+15550100"
  },
  "payment": {
    "instruments": [
      {
        "id": "<saved-payment-instrument-id>",
        "selected": true,
        "billing_address": {
          "street_address": "123 Example Street",
          "address_locality": "Exampleville",
          "address_region": "CA",
          "postal_code": "94043",
          "address_country": "US"
        }
      }
    ]
  }
}'
```

`billing_address` on the selected saved payment instrument is optional. You can provide it
when creating the checkout or in the completion request; omitting it retains the saved
instrument's existing billing address.

Use `checkout get <checkout-id>` when you need to inspect an existing open checkout or
recover its available payment instruments. Its human output shows checkout status, items,
totals, and the selected masked payment method. Use `--output json` for the full response.

## Optionally update an open checkout

`checkout update` is optional. Use it only to change an existing checkout. It replaces the
checkout with the supplied document, so include every line item and all retained fields. An empty
`line_items` array deliberately clears the cart.

```bash
gddy shopping checkout update <checkout-id> --file update-checkout.json
```

## Complete a checkout

`checkout complete` places a real order. Provide exactly one selected saved payment
instrument. You can supply a non-empty `idempotency_key`, or omit it to let gddy generate
one and return it in human output. Preserve the effective key for lost-response recovery.

```json
{
  "payment": {
    "instruments": [
      {
        "id": "<saved-payment-instrument-id>",
        "selected": true,
        "billing_address": {
          "street_address": "123 Example Street",
          "address_locality": "Exampleville",
          "address_region": "CA",
          "postal_code": "94043",
          "address_country": "US"
        }
      }
    ]
  },
  "idempotency_key": "<optional-stable-key>"
}
```

```bash
gddy shopping checkout complete <checkout-id> \
  --file complete-checkout.json
```

Completion returns immediately after the single purchase attempt. A successful response includes
the order ID and a **View order** permalink for the customer's account. It never automatically
retries. If the result is uncertain, first confirm the outcome; only then reuse the same
effective idempotency key for the same intended purchase.

New orders normally become available 3–10 seconds after completion. Retrieve the order
separately, optionally polling for up to 15 seconds by default:

```bash
gddy shopping order get <order-id> --wait --wait-timeout 15
```

The engine-wide `--timeout` remains independent of order visibility waiting:

```bash
gddy --timeout 30s shopping order get <order-id> \
  --wait --wait-timeout 15
```

After completion, use `shopping order get` with the returned order ID. `shopping checkout get`
is for open checkout sessions only.
