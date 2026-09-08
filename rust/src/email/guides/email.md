---
summary: GoDaddy Business Email product and how to use the GoDaddy CLI to create and manage mailboxes
---

# Create GoDaddy Business Email with `gddy`

This guide explains the GoDaddy Business Email product and how to use the GoDaddy CLI to create, and manage mailboxes.

Creating a mailbox is a **three-step** flow:

1. `gddy email check-eligibility --email <emailAddress>` — verify the address is eligible, find which account to use, and see what consents are required.
2. `gddy email create --email <emailAddress>` — submit the provisioning request. Returns immediately with a mailbox ID.
3. `gddy email get <mailboxId>` — poll until `status` is `COMPLETED`.

## Key concepts



### Accounts

An **account** (`accountId`) identifies an existing GoDaddy Business Email plan you already hold. It is separate from your GoDaddy login and unrelated to domain or hosting "accounts" elsewhere in `gddy` — think of it as a container that can hold a mailbox. You may have zero, one, or several eligible accounts (for example, if you have bought more than one email plan), so `email create` needs to know which one to provision the new mailbox under.

When an account has `default: true` it is the recommended choice. Use it when you have no other preference.

## Creating a Mailbox

### Step 1 — Check eligibility

Before creating a mailbox, verify the email address is eligible and discover which email plan can be used. The domain must be owned by the authenticated GoDaddy shopper.

```
gddy email check-eligibility --email someone@example.com
```

**Success (email address is eligible)** — the command returns an `EligibilityResult`:

```json
{
  "isEligible": true,
  "eligibleAccounts": [
    {
      "accountId": "00000000-0000-0000-0000-000000000001",
      "mailboxType": "TITAN",
      "accountName": "Professional Email",
      "default": true,
      "requirements": [
        {
          "type": "FREETRIAL_AUTORENEW",
          "title": "Email auto renew",
          "reference": "By creating your email you agree that after your trial ends on January 1, 2026, your email will auto-renew for $2.99/mo. Cancel anytime in Account Settings."
        }
      ]
    }
  ]
}
```

Key fields:


| Field                             | Description                                                                                    |
| --------------------------------- | ---------------------------------------------------------------------------------------------- |
| `eligibleAccounts[].accountId`    | Pass to `--account-id` on `email create`.                                                      |
| `eligibleAccounts[].default`      | `true` on the recommended account. Use this one when no specific preference.                   |
| `eligibleAccounts[].requirements` | Legal agreements you must accept. Each `type` must be passed as `--consent` on `email create`. |


When the account has **no free-trial** plan, `requirements` is an empty array and no `--consent` flag is needed:

```json
{
  "isEligible": true,
  "eligibleAccounts": [
    {
      "accountId": "00000000-0000-0000-0000-000000000002",
      "mailboxType": "TITAN",
      "accountName": "Professional Email Pro Plus",
      "default": true,
      "requirements": []
    }
  ]
}
```

**Failure (address cannot be provisioned)** — the command returns a 422 error with a `details` array explaining why. See [Eligibility failure reasons](#eligibility-failure-reasons) for the full list.

### Step 2 — Create the mailbox

Pass the chosen `accountId` and one `--consent` for each `requirements[].type`:

```
gddy email create --email someone@example.com \
  --account-id 00000000-0000-0000-0000-000000000001 \
  --consent FREETRIAL_AUTORENEW
```

`--consent` is repeatable — pass one per required requirement type. `FREETRIAL_AUTORENEW` is currently the only requirement type the API issues. When `requirements` is empty, omit `--consent` entirely.

`--account-id` is optional when there is exactly one eligible account with `default: true`; the CLI will use it automatically.

`--first-name` and `--last-name` are optional; they set the display name on the mailbox.

The command returns `202 Accepted` immediately with the new mailbox at `status: CONFIRMED` and the mailbox ID. The mailbox is **not yet ready to use** — provisioning continues in the background; poll with `gddy email get` until `status` reaches `COMPLETED` (success) or `FAILED` (terminal error). Recommended poll interval is 2–4 seconds; do not poll faster than once per second.

**Example response (with consents):**

```json
{
  "mailboxId": "73e99614-0db5-46b1-8ea7-b5228a1fe7a6",
  "emailAddress": "someone@example.com",
  "mailboxType": "TITAN",
  "firstName": "Jane",
  "lastName": "Smith",
  "displayName": "Jane Smith",
  "status": "CONFIRMED",
  "createdAt": "2026-09-02T17:51:29Z",
  "modifiedAt": "2026-09-02T17:51:29Z",
  "agreements": [
    {
      "type": "FREETRIAL_AUTORENEW",
      "agreed": true
    }
  ],
  "links": [
    {
      "rel": "self",
      "href": "/v1/email/mailboxes/73e99614-0db5-46b1-8ea7-b5228a1fe7a6"
    }
  ]
}
```

**Example response (no consents required):**

```json
{
  "mailboxId": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "emailAddress": "someone@example.com",
  "mailboxType": "TITAN",
  "firstName": "Jane",
  "lastName": "Smith",
  "displayName": "Jane Smith",
  "status": "CONFIRMED",
  "createdAt": "2026-09-02T17:51:29Z",
  "modifiedAt": "2026-09-02T17:51:29Z",
  "links": [
    {
      "rel": "self",
      "href": "/v1/email/mailboxes/a1b2c3d4-e5f6-7890-abcd-ef1234567890"
    }
  ]
}
```



### Step 3 — Poll until ready

Use the `mailboxId` from the create response:

```
gddy email get 73e99614-0db5-46b1-8ea7-b5228a1fe7a6
```

Repeat until `status` in the response is `COMPLETED` or `FAILED`. Typical provisioning takes several seconds to a minute. On `FAILED`, there is no automatic retry — the creation request must be resubmitted if appropriate.

**Example response (**`COMPLETED`**):**

```json
{
  "mailboxId": "73e99614-0db5-46b1-8ea7-b5228a1fe7a6",
  "emailAddress": "someone@example.com",
  "mailboxType": "TITAN",
  "firstName": "Jane",
  "lastName": "Smith",
  "displayName": "Jane Smith",
  "status": "COMPLETED",
  "createdAt": "2026-09-02T17:51:29Z",
  "modifiedAt": "2026-09-02T17:51:50Z",
  "links": [
    {
      "rel": "self",
      "href": "/v1/email/mailboxes/73e99614-0db5-46b1-8ea7-b5228a1fe7a6"
    }
  ]
}
```



## Error handling



### Eligibility failure reasons

When `check-eligibility` returns a 422, the `details` array contains one or more of the following `issue` codes.


| `issue`                          | When it occurs                                                                         | Recommended action                                                                                                |
| -------------------------------- | -------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `EMAIL_PLAN_NOT_ELIGIBLE`        | The domain is linked to an email plan that does not support provisioning via this API. | Go to the [GoDaddy Email dashboard](https://productivity.godaddy.com/addnewemail) to create the mailbox manually. |
| `DOMAIN_IN_OTHER_EMAIL_PROVIDER` | The domain is already provisioned through a different email provider.                  | The domain cannot be used with GoDaddy Business Email. No action available via the API.                           |
| `DOMAIN_NOT_ELIGIBLE`            | The domain exists but is not eligible for API provisioning.                            | Go to the [GoDaddy Email dashboard](https://productivity.godaddy.com/addnewemail).                                |
| `EMAIL_PLAN_NOT_AVAILABLE`       | There is no active email plan for this domain.                                         | Purchase an email plan before creating a mailbox.                                                                 |
| `EMAIL_ADDRESS_INVALID`          | The username portion fails format or length validation.                                | Fix the address — see [Username rules](#username-rules).                                                          |
| `EMAIL_ADDRESS_ALREADY_EXISTS`   | A mailbox with this address already exists.                                            | The address is taken; choose a different username.                                                                |




### Create failure reasons (422 from `gddy email create`)

The create command re-runs the eligibility check internally. A 422 can occur even if a prior `check-eligibility` succeeded, if domain state changed between the two calls.


| `issue`                          | When it occurs                                                                                  |
| -------------------------------- | ----------------------------------------------------------------------------------------------- |
| `EMAIL_PLAN_NOT_ELIGIBLE`        | Domain's plan does not support provisioning via this API.                                       |
| `DOMAIN_IN_OTHER_EMAIL_PROVIDER` | Domain is provisioned through a different email provider.                                       |
| `DOMAIN_NOT_ELIGIBLE`            | Domain exists but is not eligible for API provisioning.                                         |
| `EMAIL_PLAN_NOT_AVAILABLE`       | No active email plan for this domain.                                                           |
| `CONSENT_NOT_PROVIDED`           | A required agreement was not included in `--consent`. The `description` names the missing type. |
| `EMAIL_ADDRESS_INVALID`          | Username format or length is invalid.                                                           |
| `EMAIL_ADDRESS_ALREADY_EXISTS`   | A mailbox with this address already exists.                                                     |


If `CONSENT_NOT_PROVIDED` appears, re-run `check-eligibility` to get the current requirements list, then resubmit `create` with all required consent types.

### Username rules

The username (the part before `@`) must:

- Contain only letters (`a–z`, `A–Z`), digits (`0–9`), periods (`.`), underscores (`_`), and hyphens (`-`).
- Not start or end with a period or hyphen.
- Not contain consecutive periods (`..`).
- Not contain spaces.
- Not exceed 30 characters, or a shorter limit when the domain name is long enough that the full address would exceed 64 characters.



### Other HTTP errors


| Code | Meaning                                                                   |
| ---- | ------------------------------------------------------------------------- |
| 400  | Malformed request — missing required field or bad parameter.              |
| 401  | Access token is missing, expired, or invalid.                             |
| 403  | Token is valid but does not have permission for this resource.            |
| 404  | The requested mailbox does not exist or belongs to a different account.   |
| 409  | A mailbox with the requested email address already exists.                |
| 429  | Rate limit exceeded. Retry after the seconds in the `Retry-After` header. |




## Command reference

- `gddy email check-eligibility --email <email>` — see which accounts (if any) can
receive a new mailbox for this address, and what consent is outstanding.
- `gddy email create --email <email> [--account-id <id>] [--first-name <name>] [--last-name <name>] [--consent <requirement-type>]...` — submit a provisioning request. Returns 202 with the mailbox at `status: CONFIRMED`; poll with `gddy email get` until `COMPLETED` or `FAILED`.
- `gddy email get <mailbox-id>` — look up one mailbox by ID. Use to poll provisioning status.
- `gddy email list [--status <status>] [--field <fields>] [--page <n>] [--page-size <n>] [--total-required]` — list your mailboxes.
  - `--status`: filter by lifecycle status (`COMPLETED`, `CONFIRMED`, `FAILED`).
  - `--field`: comma-separated list of fields to include (sparse fieldset).
  - `--page`: page number, 1-based (default `1`).
  - `--page-size`: results per page, max 100 (default `25`).
  - `--total-required`: include `totalItems`, `totalPages`, and a `rel=last` link in the response.

