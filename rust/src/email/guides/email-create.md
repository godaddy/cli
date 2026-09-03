---
summary: How GoDaddy Email mailboxes, accounts, and consent fit together
---

# GoDaddy Email

This guide explains the GoDaddy email system and how to use the `gddy email` commands to check
eligibility for, create, and manage mailboxes.

## What an "account" is here

An **account** (`accountId`) identifies an existing GoDaddy Email account you
already hold. It's separate from your GoDaddy login and has nothing to do
with a domain name or with domain/hosting "accounts" elsewhere in `gddy` —
think of it as a container that can hold multiple mailboxes. You may have
zero, one, or several eligible email accounts (for example, if you've bought
more than one email plan), so `email create` needs to know which one to
provision the new mailbox under.

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
  "ineligibilityReasons": [
    { "type": "NO_ELIGIBLE_ACCOUNT", "message": "No eligible account was found." }
  ],
  "eligibleAccounts": [
    {
      "accountId": "acct-123",
      "requirements": [
        {
          "type": "FREETRIAL_AUTORENEW",
          "title": "Email auto renew",
          "reference": "By continuing, you agree this mailbox will auto-renew..."
        }
      ]
    }
  ]
}
```

Pass the account you chose, and the agreements you're accepting, straight
into `create`:

```
gddy email create --email someone@example.com \
  --account-id acct-123 \
  --consent FREETRIAL_AUTORENEW
```

`--consent` is repeatable — pass one per required requirement `type`.
`FREETRIAL_AUTORENEW` is currently the only requirement type the API issues.
If `create` fails with a `400`/`422` about missing agreements or no eligible
account, re-run `check-eligibility` to see the current requirements.

## Command reference

- `gddy email check-eligibility --email <email>` — see which accounts (if
  any) can receive a new mailbox for this address, and what consent is
  outstanding.
- `gddy email create --email <email> [--account-id] [--first-name]
  [--last-name] [--consent <requirement-type>]...` — provision a mailbox.
- `gddy email list [--status] [--fields] [--limit] [--offset]` — list your
  mailboxes.
- `gddy email get <mailbox-id>` — look up one mailbox by ID.
