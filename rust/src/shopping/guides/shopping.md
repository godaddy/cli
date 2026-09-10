---
summary: Browse products, place orders, and retrieve purchase details.
---

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

Shopping requests use the selected environment's standard API front door. OAuth requests the
complete lifecycle scope bundle above in one consent flow. PAT support requires those Shopping
scopes to be available on the Developer Portal and is tracked separately.

## Environment

Commands below use the active environment. To run them against another configured
environment, add `--env <environment>` before `shopping`:

```bash
gddy --env <environment> shopping catalog search
```

## Request documents and output

Use `--body` for a small inline JSON request or `--file` for a reusable JSON document.
`--file` takes precedence over `--body`. For common checkout workflows, use the checkout
flags below; use JSON for advanced nested API fields. Do not combine checkout flags with
`--body` or `--file` in the same command.

JSON is the default output format. For successful non-dry-run requests, the full
Shopping API response is in the envelope's `data` field. Add `--output human` for a
concise terminal presentation, or `--output json` to make the default explicit. Use
`gddy shopping <command> --help` for command-specific requirements.

## Discover products

Search the catalog without a request body. Use `--query`, repeatable `--category`, `--cursor`,
`--limit`, `--currency`, and `--country` to refine a search:

```bash
gddy shopping catalog search \
  --query email \
  --category email \
  --currency GBP \
  --country GB \
  --limit 3
```

`--currency` writes `context.currency` into the request. The API response price currency
is authoritative; a requested currency is a preference, not a guarantee. The current
Shopping service applies the preference to catalog search; catalog lookup and product
retrieval currently accept the context but can return their default USD prices. Use
`--body` or `--file` for advanced filters and extensions. A raw `filters.price` request
must include `context.currency`, because its bounds are currency-specific minor units.

`catalog search` displays products as numbered sections. Each section shows its product
ID, then purchasable variants and prices. `--limit` controls **products**, not variants.
Use a **product ID** with `catalog get` or `catalog lookup`; use a **variant ID** in
`checkout create` line items. A variant is already term-specific—select the variant whose
returned **Term** column matches the desired term.

```bash
gddy shopping catalog lookup \
  --id nes-wsb-vnext-tier1 \
  --currency GBP

gddy shopping catalog get \
  --id nes-wsb-vnext-tier1 \
  --currency GBP
```

To receive the full catalog response as valid JSON:

```bash
gddy --output json shopping catalog search --limit 3
```

Use the opaque response cursor with the same search criteria:

```bash
gddy shopping catalog search \
  --query email \
  --currency GBP \
  --limit 3 \
  --cursor '<cursor from previous response>'
```

For advanced filters or extensions not represented by command flags, continue using a JSON
request through `--body` or `--file`.

## Create a checkout ready to complete

Creating a checkout does not place an order. A create response includes the checkout ID,
priced line items, totals, and available payment instruments, so a separate `checkout get`
is not required before completion when the checkout is already ready. Include buyer, payment,
and other supported checkout information when creating a checkout that is ready to complete.

```bash
gddy shopping checkout create \
  --item '<available-variant-id>' \
  --currency USD \
  --buyer-first-name Jane \
  --buyer-last-name Doe \
  --buyer-email jane.doe@example.test \
  --buyer-phone '+15550100'
```

Repeat `--item` to create a cart; append `=QUANTITY` to an item, such as
`--item '<available-variant-id>=2'`. Add `--payment-instrument <saved-payment-instrument-id>`
to select one stored payment method. It is optional at creation: a ready checkout can expose a
saved instrument for selection at completion.

A stored payment instrument normally supplies its saved billing address automatically. Use a
JSON document when you need an address override or other advanced nested fields:

```json
{
  "line_items": [
    {
      "item": {"id": "<catalog-variant-id>"},
      "quantity": 1,
      "input": {
        "type": "<variant-required-input-type>",
        "references": {"<reference-name>": "<reference-value>"}
      }
    }
  ],
  "buyer": {
    "first_name": "Jane",
    "last_name": "Doe",
    "email": "jane.doe@example.test",
    "phone_number": "+15550100"
  },
  "context": {
    "currency": "USD"
  },
  "payment": {
    "instruments": [{
      "id": "<saved-payment-instrument-id>",
      "selected": true,
      "billing_address": {
        "street_address": "123 Example Street",
        "extended_address": "Suite 200",
        "address_locality": "Exampleville",
        "address_region": "CA",
        "postal_code": "94043",
        "address_country": "US",
        "first_name": "Jane",
        "last_name": "Doe",
        "phone_number": "+15550100"
      }
    }]
  }
}
```

`line_items` is the only required top-level field for create or update. Each line item requires
an `item.id` (a catalog variant ID) and a positive integer `quantity`. Include `input` only when
the selected variant's published input schema requires it. `buyer`, `context`, `signals`,
`attribution`, `payment`, and `fulfillment` are optional. Do not send response-owned fields such
as checkout `id`, `status`, `totals`, `currency`, `messages`, `order`, or `ucp`.

Only one payment instrument may be specified for checkout create, update, or complete. Use
`--file checkout.json` rather than placing address information in shell history.

Use `checkout get <checkout-id>` when you need to inspect an existing open checkout or
recover its available payment instruments. Its human output shows checkout status, items,
totals, and the selected masked payment method. Use `--output json` for the full response.

## Optionally update an open checkout

`checkout update` is optional. Use it only to change an existing checkout. It replaces the
checkout state, so structured updates must include every desired cart item. Use `--clear-items`
only to deliberately empty the cart.

```bash
gddy shopping checkout update <checkout-id> \
  --item '<available-variant-id>=2' \
  --buyer-email jane.doe@example.test
```

Use `--file update-checkout.json` for advanced replacement fields, such as fulfillment, product
input, attribution, signals, or a billing-address override.

## Complete a checkout

`checkout complete` places a real order. Select exactly one saved payment instrument. The
common form is:

```bash
gddy shopping checkout complete <checkout-id> \
  --payment-instrument <saved-payment-instrument-id>
```

Use `--idempotency-key <optional-stable-key>` to supply a non-empty key, or omit it to let
gddy generate one and return it in human output. Preserve the effective key for lost-response
recovery.

Use `--file complete-checkout.json` for an optional billing-address override or another
advanced payment field. The JSON may specify only one payment instrument:

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
