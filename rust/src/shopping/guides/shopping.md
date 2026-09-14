---
summary: Find GoDaddy products, create a checkout session with products, complete purchase, and review orders.
---

# Shopping for GoDaddy Products

Use `gddy shopping` to find GoDaddy products, add selected purchase options to a checkout session, place an order, and review completed purchases.

## Concepts

- A **product** can have multiple **purchase options**. Choose the purchase option with the features, term, and price you want.
- A **checkout session** is your cart. It holds the purchase options you intend to purchase, and the `shopping checkout` commands manage it.
- A checkout session can show eligible saved **payment methods**. Their availability can depend on the checkout session, including its currency.
- A checkout session includes important links, such as terms, privacy, refund, shipping, or help information. Review every listed link before placing an order.
- Complete a checkout session to place an **order**.

## Find a product

Search all available products or use a text query:

```bash
gddy shopping catalog search
gddy shopping catalog search --query hosting --currency GBP --limit 3
```

Use `--category` to narrow the results. It can be repeated when more than one category applies:

```bash
gddy shopping catalog search --category webHosting --category email
```

`--limit` controls the number of products, not the number of purchase options. Use a product ID with `catalog get` or `catalog lookup`; use a purchase option ID when creating a checkout session. A requested currency is a preference—the currency returned with the price is authoritative.

```bash
gddy shopping catalog get <product-id> --currency GBP
gddy shopping catalog lookup <product-id> --currency GBP
```

Use `--output json` when you need the complete command output:

```bash
gddy --output json shopping catalog search --query hosting
```

## Create and review a checkout session

Create a checkout session with one selected purchase option. Add buyer details when they are needed for the product or payment method:

```bash
gddy shopping checkout create \
  --item <purchase-option-id> \
  --currency GBP \
  --buyer-first-name Jane \
  --buyer-last-name Doe \
  --buyer-email jane.doe@example.com
```

Repeat `--item` to add purchase options. Append `=QUANTITY` when you need more than one of a purchase option:

```bash
gddy shopping checkout create --item <purchase-option-id>=2
```

The response shows checkout-session items, its selected and available payment methods, the final total when available, and all important links. It lists five payment methods by default; add `--show-all-payment-instruments` to show every available method.

If you need to add a payment method, run the following command. It opens the payment-method page in your browser. Then retrieve the checkout session again to see payment methods eligible for that checkout session.

```bash
gddy payment-methods add
```

```bash
gddy shopping checkout get <checkout-id> --show-all-payment-instruments
```

To select an eligible saved payment method explicitly, provide its ID when creating or updating the checkout session. Only one payment method can be selected.

```bash
gddy shopping checkout update <checkout-id> \
  --item <purchase-option-id> \
  --payment-instrument <payment-instrument-id>
```

Updating a checkout session is optional. It replaces the checkout session's writable contents, so include every item you want to keep. Use `--clear-items` only when you intend to empty the checkout session.

## Place an order

Review the checkout session, its payment method, and every important link first. Then place the order with `--agree` to acknowledge the links:

```bash
gddy shopping checkout complete <checkout-id> --agree
```

If a payment method is already selected on the checkout session, `--payment-instrument` can be omitted. To select an eligible saved payment method explicitly:

```bash
gddy shopping checkout complete <checkout-id> \
  --payment-instrument <payment-instrument-id> \
  --agree
```

To provide a billing address, use `--billing-address` with a JSON object containing one or more of these fields: `first_name`, `last_name`, `phone_number`, `street_address`, `extended_address`, `address_locality` (city or locality), `address_region` (state or province), `postal_code`, and `address_country`.

```bash
gddy shopping checkout complete <checkout-id> --agree \
  --billing-address '{
    "street_address":"123 Main St",
    "address_locality":"Mountain View",
    "address_region":"CA",
    "postal_code":"94043",
    "address_country":"US"
  }'
```

## Review an order

A newly placed order can take a few seconds to become available. Use `--wait` to check until it appears:

```bash
gddy shopping order get <order-id> --wait --wait-timeout 15
```

Use `shopping order get` for completed purchases. Do not use `shopping checkout get` after an order is placed.
