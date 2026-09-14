---
summary: Deploy a Node.js app to GoDaddy hosting — provision, upload, preview, then publish
---

# `gddy hosting` — deploying a Node.js application

`gddy hosting` manages the full lifecycle of a hosted Node.js application: create an app slot,
upload source code, test on a staging URL, attach a billing plan, and publish to production.

Every application has two environments: **PREVIEW** (staging) and **PUBLISH** (production). Source
uploads always land on PREVIEW first; `deployment publish` promotes the current PREVIEW build to
PUBLISH.

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

Choose one approach.

**From a local zip archive:**

```sh
gddy hosting source upload --app-id <app-id> --file ./app.zip
```

**From a GitHub branch** (connect GitHub at godaddy.com first):

```sh
gddy hosting source github --app-id <app-id> --repo owner/repo --branch main
```

In either case, poll the import until COMPLETED:

```sh
gddy hosting source status --app-id <app-id> --import-id <import-id>
```

## 3. Test on PREVIEW

Once the import is COMPLETED the app is live on its PREVIEW URL. Retrieve it:

```sh
gddy hosting app get --app-id <app-id>
```

The `urls` field shows the reachable address for each environment.

## 4. Attach a subscription (first deploy only)

A hosting plan subscription is required before publishing. Check whether one is already attached:

```sh
gddy hosting subscription get --app-id <app-id>
```

If none is attached yet, list available plans and attach one:

```sh
gddy hosting subscription list
gddy hosting subscription attach --app-id <app-id> --subscription-id <subscription-id>
```

The subscription stays attached — skip this step on subsequent deploys.

## 5. Publish to production

Promote the current PREVIEW build to PUBLISH:

```sh
gddy hosting deployment publish --app-id <app-id>
```

Poll until COMPLETED:

```sh
gddy hosting deployment get --app-id <app-id> --deployment-id <deployment-id>
```

## Redeploying

Upload source again (step 2), wait for the import to complete, then run `deployment publish`
again (step 5). Skip steps 1 and 4.

## Other commands

| Command | Purpose |
|---|---|
| `hosting app status` | Runtime status for PREVIEW and PUBLISH |
| `hosting app restart --variant <PREVIEW\|PUBLISH>` | Restart an environment |
| `hosting log list` | Fetch log entries (filter by `--variant`, `--level`, `--since`) |
| `hosting secrets create/update/delete/list` | Manage per-environment secrets |
| `hosting domain attach/detach/list` | Manage custom domains |
| `hosting runtime get` | View the Node.js runtime version |

## See also

- `gddy guide auth` — how authentication and credential storage work.
