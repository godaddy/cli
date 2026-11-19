---
summary: GoDaddy Business Email product and how to use the GoDaddy CLI to create and manage mailboxes
---
# Create GoDaddy Business Email with `gddy`

This guide explains the GoDaddy Business Email product and how to use the GoDaddy CLI to create, and manage mailboxes.

Creating a mailbox takes three commands: check eligibility, create the mailbox, and poll until it's ready. Go to [Create a mailbox](#create-a-mailbox) for the steps.

## Concepts

The following terms and concepts are relevant to creating and managing mailboxes:

**Mailbox**: a single email address (for example, `jane@example.com`) backed by GoDaddy Business Email. Each mailbox has its own credentials and storage. Provisioning a mailbox is asynchronous. The create command returns immediately and the mailbox becomes usable once `status` reaches `COMPLETED`.

**Email plan or account**: an existing GoDaddy Business Email plan you already hold, identified by `accountId`. Think of it as a container that can hold a mailbox. You might have zero, one, or several eligible accounts (for example, if you have bought more than one email plan). So `email create` needs to know which one to provision the new mailbox under. When an account has `default: true` it's the recommended choice. Use it when you have no other preference.

## Create a mailbox

To create a mailbox, run three commands in order:

1. Check eligibility to verify the address, find which account to use, and see which consents you need to give:

   ```
   gddy email check-eligibility --email <emailAddress>
   ```

2. Create the mailbox to submit the provisioning request, which returns a mailbox ID right away:

   ```
   gddy email create --email <emailAddress>
   ```

3. Poll the mailbox until `status` is `COMPLETED`:

   ```
   gddy email get <mailboxId>
   ```

### Check eligibility

Before you create a mailbox, check that the email address is eligible and find which email plan to use. The domain must be owned by the authenticated GoDaddy shopper.

The result lists each eligible account with these fields:

| Field                             | Description                                                                                    |
| --------------------------------- | ---------------------------------------------------------------------------------------------- |
| `eligibleAccounts[].accountId`    | Pass to `--account-id` on `email create`.                                                      |
| `eligibleAccounts[].default`      | `true` on the recommended account. Use this one when no specific preference.                   |
| `eligibleAccounts[].requirements` | Legal agreements you must accept. Each `type` must be passed as `--consent` on `email create`. The array is empty when the account has no free-trial plan, and then you don't need `--consent`. |

1. Run the eligibility check:

   ```
   gddy email check-eligibility --email someone@example.com
   ```

2. Read the result, which lists each eligible account (go to [Eligibility result](#eligibility-result) for an example).

3. If the command fails with a 422 error, its message lists each reason as an issue code (go to [Eligibility failure reasons](#eligibility-failure-reasons) for the full list).

### Create the mailbox

After you've checked eligibility, create the mailbox with the account and consents you chose. Three of the options need a note:

- `--consent` is repeatable. Pass one per required requirement type. `FREETRIAL_AUTORENEW` is the only requirement type the API issues. When `requirements` is empty, omit `--consent` entirely.
- `--account-id` is optional when there is exactly one eligible account with `default: true`; the CLI will use it automatically.
- `--first-name` and `--last-name` are optional; they set the display name on the mailbox.

1. Run the create command with the chosen `accountId` and one `--consent` for each `requirements[].type`:

   ```
   gddy email create --email someone@example.com \
     --account-id 00000000-0000-0000-0000-000000000001 \
     --consent FREETRIAL_AUTORENEW
   ```

2. Read the response, which comes back right away with the mailbox ID and `status: EXECUTING` (go to [Mailbox response](#mailbox-response) for an example).

The mailbox isn't ready to use yet. Provisioning continues in the background; poll with `gddy email get` until `status` reaches `COMPLETED` (success) or `FAILED` (terminal error). Recommended poll interval is 2–4 seconds; don't poll faster than once per second.

### Poll until ready

Typical provisioning takes several seconds to a minute. The response has the same fields as the [Mailbox response](#mailbox-response) from `create`. On `FAILED`, there is no automatic retry. Resubmit the creation request if appropriate.

1. Run `gddy email get` with the `mailboxId` from the create response:

   ```
   gddy email get 73e99614-0db5-46b1-8ea7-b5228a1fe7a6
   ```

2. Repeat until `status` in the response is `COMPLETED` or `FAILED`.

## Example responses

These examples show the `data` field of the CLI's JSON output. The CLI also prints `next_actions`, a list of suggested follow-up commands.

### Eligibility result

`check-eligibility` returns this for an address with one eligible account that has a free-trial agreement. For an account with no free-trial plan, `requirements` is an empty array.

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

### Mailbox response

`create` returns this with `status: EXECUTING`. `get` returns the same fields, with `status` set to `COMPLETED` or `FAILED` and a later `updatedAt`. If you pass no `--consent`, `agreements` is left out.

```json
{
  "mailboxId": "73e99614-0db5-46b1-8ea7-b5228a1fe7a6",
  "emailAddress": "someone@example.com",
  "mailboxType": "TITAN",
  "firstName": "Jane",
  "lastName": "Smith",
  "displayName": "Jane Smith",
  "status": "EXECUTING",
  "createdAt": "2026-09-02T17:51:29Z",
  "updatedAt": "2026-09-02T17:51:29Z",
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

## Error handling

These sections cover why the eligibility check or the create command can fail, the rules for the username in an email address, and other HTTP errors.

### Eligibility failure reasons

When `check-eligibility` fails with a 422, the error message lists one or more of the following `issue` codes.

| `issue`                          | When it occurs                                                                         | Recommended action                                                                                                 |
| -------------------------------- | -------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `EMAIL_PLAN_NOT_ELIGIBLE`        | The domain is linked to an email plan that doesn't support provisioning through this API. | Go to the [GoDaddy Email dashboard](https://productivity.godaddy.com/addnewemail) to create the mailbox manually.  |
| `DOMAIN_IN_OTHER_EMAIL_PROVIDER` | The domain is already provisioned through a different email provider.                  | The domain can't be used with GoDaddy Business Email. No action is available through the API.                            |
| `DOMAIN_NOT_ELIGIBLE`            | The domain exists but isn't eligible for API provisioning.                             | Go to the [GoDaddy Email dashboard](https://productivity.godaddy.com/addnewemail).                                 |
| `EMAIL_PLAN_NOT_AVAILABLE`       | There is no active email plan for this domain.                                         | Run `gddy shopping catalog search --query titan` to see available email plans, select one to purchase, then retry. |
| `EMAIL_ADDRESS_INVALID`          | The username portion fails format or length validation.                                | Fix the address. Go to [Username rules](#username-rules) for the rules.                                                           |
| `EMAIL_ADDRESS_ALREADY_EXISTS`   | A mailbox with this address already exists.                                            | The address is taken; choose a different username.                                                                 |

### Create failure reasons (422 from `gddy email create`)

The create command re-runs the eligibility check internally. A 422 can occur even if a prior `check-eligibility` succeeded, if domain state changed between the two calls.

| `issue`                          | When it occurs                                                                                                     |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `EMAIL_PLAN_NOT_ELIGIBLE`        | Domain's plan doesn't support provisioning through this API.                                                          |
| `DOMAIN_IN_OTHER_EMAIL_PROVIDER` | Domain is provisioned through a different email provider.                                                          |
| `DOMAIN_NOT_ELIGIBLE`            | Domain exists but isn't eligible for API provisioning.                                                            |
| `EMAIL_PLAN_NOT_AVAILABLE`       | Run `gddy shopping catalog search --query titan` to see available email plans, select one to purchase, then retry. |
| `CONSENT_NOT_PROVIDED`           | A required agreement wasn't included in `--consent`. The `description` names the missing type.                    |
| `EMAIL_ADDRESS_INVALID`          | Username format or length is invalid.                                                                              |
| `EMAIL_ADDRESS_ALREADY_EXISTS`   | A mailbox with this address already exists.                                                                        |

If `CONSENT_NOT_PROVIDED` shows, re-run `check-eligibility` to get the current requirements list, then resubmit `create` with all required consent types.

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
| 400  | Malformed request: missing required field or bad parameter.              |
| 401  | Access token is missing, expired, or invalid.                             |
| 403  | Token is valid but doesn't have permission for this resource.            |
| 404  | The requested mailbox doesn't exist or belongs to a different account.   |
| 409  | A mailbox with the requested email address already exists.                |
| 429  | Rate limit exceeded. Retry after the seconds in the `Retry-After` header. |

## Command reference

- `gddy email check-eligibility --email <email>`: see which accounts (if any) can receive a new mailbox for this address, and what consent is outstanding.
- `gddy email create --email <email> [--account-id <id>] [--first-name <name>] [--last-name <name>] [--consent <requirement-type>]...`: submit a provisioning request. Returns right away with the mailbox at `status: EXECUTING`; poll with `gddy email get <mailbox-id>` until `COMPLETED` or `FAILED`.
- `gddy email get <mailbox-id>`: look up one mailbox by ID. Use to poll provisioning status.
- `gddy email list [--status <status>] [--field <fields>] [--page <n>] [--page-size <n>] [--total-required]`: list your mailboxes.
  - `--status`: filter by lifecycle status (`COMPLETED`, `EXECUTING`, `FAILED`).
  - `--field`: comma-separated list of fields to include (sparse fieldset).
  - `--page`: page number, 1-based (default `1`).
  - `--page-size`: results per page, max 100 (default `25`).
  - `--total-required`: include `totalItems`, `totalPages`, and a `rel=last` link in the response.