---
summary: Deploy a Node.js app to GoDaddy hosting — provision, upload, preview, then publish
---

# `gddy hosting` — deploying a Node.js application

`gddy hosting` manages the full lifecycle of a hosted Node.js application: create an app, upload source code, test on a staging URL, attach to a hosting plan, and publish to production.

## Concepts

**App** — the hosted Node.js application. Create it first. Later commands take `--app-id`.

**Environments** — each app has PREVIEW (staging) and PUBLISH (production). The CLI calls this `variant`. Uploads land on PREVIEW. `deployment publish` promotes that build to PUBLISH.

**Subscription** — the API name for a hosting plan you already bought (resources and billing). Attach the app to one subscription before the first publish.

**Slot** — available space within a subscription for an attached app. Each app attached to a subscription takes up one slot, and a given subscription can have one or more slots.

## 1. Create an application

App creation is asynchronous. The command returns an operation ID immediately:

```sh
gddy hosting app create --app-type NODEJS --name my-app
```

Poll until COMPLETED, then note the `app.id` in the response:

```sh
gddy hosting operation get --operation-id <operation-id>
```

## 2. Upload source code

Deploys code to the app's PREVIEW environment. You can run this again to replace what was uploaded before.

```sh
gddy hosting source upload --app-id <app-id> --file ./app.zip
```

Poll the import until COMPLETED:

```sh
gddy hosting source status --app-id <app-id> --import-id <import-id>
```

## 3. Test on PREVIEW

Once the import is COMPLETED the app is live on its PREVIEW URL. Retrieve it:

```sh
gddy hosting app get --app-id <app-id>
```

The `urls` field shows the address for each environment.

## 4. Attach a subscription (first deploy only)

Check whether the app is already on a subscription:

```sh
gddy hosting subscription get --app-id <app-id>
```

If not, list subscriptions and attach the app to one with open slots (`availableSlots > 0`).

```sh
gddy hosting subscription list
gddy hosting subscription attach --app-id <app-id> --subscription-id <subscription-id>
```

If `totalAvailableSlots` is 0, buy a Web Hosting plan (`gddy shopping catalog search --category webHosting` for NODEJS), poll list until a slot appears, then attach. See `gddy guide shopping` for checkout.

Skip this on later deploys.

## 5. Publish to production

```sh
gddy hosting deployment publish --app-id <app-id>
```

If publish returns `WH_PLAN_REQUIRED`, the app is not attached — go back to step 4.

Poll until COMPLETED:

```sh
gddy hosting deployment get --app-id <app-id> --deployment-id <deployment-id>
```

## 6. Custom domains

Attach after the app is published:

```sh
gddy hosting domain attach --app-id <app-id> --hostname www.example.com
gddy hosting domain get --app-id <app-id> --domain-id <domain-id>
```

`domainType` on list/get is `CUSTOM` for an attached customer hostname. `PREFIX` is the platform hostname, whose prefix can be changed in the hosting UI (not using this CLI).

For a CUSTOM domain whose DNS is **not** on GoDaddy, poll `hosting domain get` and apply records at the external DNS host as fields become non-null:

| When | Record | Target |
|---|---|---|
| `certificateValidationCname` is set | CNAME `_acme-challenge` | that hostname (proves ownership / issues TLS) |
| `anycastIp` is set | A for the attached hostname | that IPv4 address (traffic to hosting) |

Keep polling until `verificationStatus` is `ACTIVE`. `cdnStatus` follows CDN provisioning (`INIT` → `PENDING` → `ACTIVE`).

## Redeploying

Upload source again (step 2), wait for the import to complete, then run `deployment publish` again (step 5). Skip steps 1 and 4.

## Other commands

| Command | Purpose |
|---|---|
| `hosting app status` | Runtime status for PREVIEW and PUBLISH |
| `hosting app restart --variant <PREVIEW\|PUBLISH>` | Restart an environment |
| `hosting log list` | Fetch log entries (filter by `--variant`, `--level`, `--since`) |
| `hosting secrets create/update/delete/list` | Manage per-environment secrets |
| `hosting domain attach/get/detach/list` | Custom domains. Get returns DNS targets for external DNS |
| `hosting runtime get` | View the Node.js runtime version |
| `hosting source github` | Deploy code from a repo already linked in the hosting UI (`source` is GitHub on `app get`) |

## See also

- `gddy guide auth` — how authentication and credential storage work.
