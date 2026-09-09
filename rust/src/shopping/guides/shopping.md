# Shopping API

`gddy shopping` is a direct integration with the Shopping API.
It is currently intended for configured non-production Katana environments; it
does not fall back to the GoDaddy front door.

## Configure the direct service

Add the service URL to `~/.config/gddy/environments.toml`:

```toml
[test]
api_url = "https://api.test-godaddy.com"
client_id = "<CLI OAuth client ID for test>"
shopping_url = "https://ecommorder-order-management-mcp-test.ecommorder-test.prod.onkatana.net"
```

For one invocation, use `TEST_SHOPPING_URL` or `SHOPPING_URL`. Per-environment
overrides take precedence over the global variable, which takes precedence over
`shopping_url` in the TOML file.

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

PATs are not currently accepted by the direct Katana service. Use OAuth until
Shopping is exposed through the front door, where PAT exchange can occur.

## Workflow

Use raw JSON (`--body`) or a JSON document (`--file`) for request bodies:

```bash
gddy --env test shopping catalog search --body '{}' --limit 3
gddy --env test shopping catalog lookup --body '{"ids":["nes-wsb-vnext-tier1"]}'
gddy --env test shopping catalog get --body '{"id":"nes-wsb-vnext-tier1"}'
gddy --env test shopping checkout create --file create-checkout.json
gddy --env test shopping checkout update <checkout-id> --file update-checkout.json
```

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

`checkout complete` places a real order. Its body must include a selected saved
payment instrument and a caller-owned, non-empty `idempotency_key`. Never create a
new idempotency key when retrying an uncertain completion; reuse the original key.

```bash
gddy --env test shopping checkout complete <checkout-id> \
  --file complete-checkout.json --wait-for-order
```

Order read models are eventually consistent and normally become visible 3–10
seconds after completion. `--wait-for-order` polls the returned order ID for up to
15 seconds by default. You can also run:

```bash
gddy --env test shopping order get <order-id> --wait --timeout 15
```

Do not call `shopping checkout get` after completion: the current service reads the
underlying open basket and does not accurately represent completed checkouts. Use
`shopping order get` instead.
