---
summary: How GoDaddy Email mailboxes, accounts, and consent fit together
---

# GoDaddy Email mailboxes

`gddy email` manages GoDaddy Email mailboxes over panel-v3, the newer
OAuth-bearer-token API. It's a different system from domain or hosting
"accounts" — this guide covers the vocabulary specific to email.

## What an "account" is here

An **account** (`accountId`) identifies an existing GoDaddy Email/productivity
account the customer already holds under panel-v3. It's an opaque ID scoped to
this API — it is **not** a shopper/customer ID, and it has nothing to do with
domain or hosting "accounts" elsewhere in `gddy`. A customer may hold zero,
one, or several eligible email accounts; `email create` needs to know which
one to provision the new mailbox under.

## The check-eligibility → create flow

Before creating a mailbox, check whether an email address is eligible and
which account(s) it can be created under:

```
gddy email check-eligibility --email someone@example.com
```

The response's `eligibleAccounts` array lists each account you can use,
together with any outstanding `requirements` (legal agreements that must be
accepted first):

```json
{
  "isEligible": false,
  "ineligibleReasons": ["NO_ELIGIBLE_ACCOUNT"],
  "eligibleAccounts": [
    {
      "accountId": "acct-123",
      "requirements": [{ "agreementType": "EMAIL_TOS", "url": "https://..." }]
    }
  ]
}
```

Pass the account you chose, and the agreements you're accepting, straight
into `create`:

```
gddy email create --email someone@example.com \
  --account-id acct-123 \
  --consent EMAIL_TOS
```

`--consent` is repeatable — pass one per required `agreementType`. If
`create` fails with a `400`/`422` about missing agreements or no eligible
account, re-run `check-eligibility` to see the current requirements.

## Command reference

- `gddy email check-eligibility --email <email>` — see which accounts (if
  any) can receive a new mailbox for this address, and what consent is
  outstanding.
- `gddy email create --email <email> [--account-id] [--first-name]
  [--last-name] [--consent <agreementType>]...` — provision a mailbox.
- `gddy email list [--status] [--fields] [--limit] [--offset]` — list your
  mailboxes.
- `gddy email get <mailbox-id>` — look up one mailbox by ID.
