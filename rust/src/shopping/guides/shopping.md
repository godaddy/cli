---
summary: Find GoDaddy products, add them to a cart, place an order, and review purchases.
---

# Shopping for GoDaddy Products

Use `gddy shopping` to find GoDaddy products, add selected product variants to a cart, place an order, and review completed purchases.

## Concepts

- A **product** can have multiple purchasable **variants**. Choose a variant that has the features and term you want.
- A **cart** holds the variants you intend to purchase. The `shopping checkout` commands manage this cart.
- A cart can show eligible saved **payment methods**. Their availability can depend on the cart, including its currency.
- A cart includes important links, such as terms, privacy, refund, shipping, or help information. Review every listed link before placing an order.
- An **order** is created when the cart is completed.

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

`--limit` controls the number of products, not the number of variants. Use a product ID with `catalog get` or `catalog lookup`; use a variant ID when creating a cart. A requested currency is a preference—the currency returned with the price is authoritative.

```bash
gddy shopping catalog get --id <product-id> --currency GBP
gddy shopping catalog lookup --id <product-id> --currency GBP
```

Use `--output json` when you need the complete command output:

```bash
gddy --output json shopping catalog search --query hosting
```

## Create and review a cart

Create a cart with one selected variant. Add buyer details when they are needed for the product or payment method:

```bash
gddy shopping checkout create \
  --item <variant-id> \
  --currency GBP \
  --buyer-first-name Jane \
  --buyer-last-name Doe \
  --buyer-email jane.doe@example.com
```

Repeat `--item` to add variants. Append `=QUANTITY` when you need more than one of a variant:

```bash
gddy shopping checkout create --item <variant-id>=2
```

The response shows cart items, its selected and available payment methods, the final total when available, and all important links. It lists five payment methods by default; add `--show-all-payment-instruments` to show every available method.

```bash
gddy shopping checkout get <checkout-id> --show-all-payment-instruments
```

To select an eligible saved payment method explicitly, provide its ID when creating or updating the cart. Only one payment method can be selected.

```bash
gddy shopping checkout update <checkout-id> \
  --item <variant-id> \
  --payment-instrument <payment-instrument-id>
```

Updating a cart is optional. It replaces the cart's writable contents, so include every item you want to keep. Use `--clear-items` only when you intend to empty the cart.

## Place an order

Review the cart, its payment method, and every important link first. Then place the order with `--agree` to acknowledge the links:

```bash
gddy shopping checkout complete <checkout-id> \
  --payment-instrument <payment-instrument-id> \
  --agree
```

If a payment method is already selected on the cart, `--payment-instrument` can be omitted.

## Review an order

A newly placed order can take a few seconds to become available. Use `--wait` to check until it appears:

```bash
gddy shopping order get <order-id> --wait --wait-timeout 15
```

Use `shopping order get` for completed purchases. Do not use `shopping checkout get` after an order is placed.
