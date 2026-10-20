---
summary: Deploy a Node.js app to GoDaddy hosting: create, upload, preview, attach to a plan, publish
---

# `gddy hosting` — deploying a Node.js application

`gddy hosting` manages the full lifecycle of a hosted Node.js application: create an app, upload source code, test on a staging URL, attach to a hosting plan, and publish to production.

## Concepts

**App** — the hosted Node.js application. Create it first. Later commands take `--app-id`.

**Environments** — each app has PREVIEW (staging) and PUBLISH (production). The CLI calls this `variant`. Uploads land on PREVIEW. `deployment publish` promotes that build to PUBLISH.

**Subscription** — a hosting plan you already bought (resources and billing). Attach the app to one before the first publish. Don't have one? See *Buy a plan*.

**Slot** — available space within a subscription for an attached app. Each app attached to a subscription takes up one slot, and a given subscription can have one or more slots.

## Create the app

App creation is asynchronous. The command returns an operation ID immediately:

```sh
gddy hosting app create --app-type NODEJS --name my-app
```

Poll until COMPLETED, then note the `app.id` in the response:

```sh
gddy hosting operation get --operation-id <operation-id>
```

## Prepare the source

The GoDaddy hosting platform is flexible, supporting a wide range of Node.js app frameworks. Source code can be deployed using a `.zip` file containing your project directory.

The top level of the project must include a `package.json` file at the top level containing:
  - A non-empty `name`
  - A `version`
  - A `main` entry point
  - A `build` script (use `echo build` if nothing needs compiling)
  - A `start` script

Any package needed during build or start must be in `dependencies`, not `devDependencies`.

The `start` script is what GoDaddy's hosting platform will invoke to run your application. When started, your application must open a listener (like a web server) to the TCP port number present in the `process.env.PORT` environment variable.

When zipping your project, exclude the `node_modules` directory.

For a full list of application requirements and instructions on preparing a `.zip` file for uploading, go to [Node.js app requirements](https://developer.godaddy.com/en/docs/api-users/hosting/app-requirements).

## Upload

Deploys code to the app's PREVIEW environment. You can run this again to replace what was uploaded before.

```sh
gddy hosting source upload --app-id <app-id> --file ./app.zip
```

Poll the import until COMPLETED:

```sh
gddy hosting source status --app-id <app-id> --import-id <import-id>
```

## Preview

Once the import is COMPLETED the app is live on its PREVIEW URL. Retrieve it:

```sh
gddy hosting app get --app-id <app-id>
```

The `urls` field shows the address for each environment.

## Attach a plan (first deploy only)

Check whether the app is already on a subscription:

```sh
gddy hosting subscription get --app-id <app-id>
```

If not, list subscriptions and attach the app to one with open slots (`availableSlots > 0`).

```sh
gddy hosting subscription list --hosting-product=WEB_HOSTING
gddy hosting subscription attach --app-id <app-id> --subscription-id <subscription-id>
```

If several plans have open slots, pick the one you want.

If `totalAvailableSlots` is 0, see *Buy a plan*.

Skip this on later deploys.

## Buy a plan

To publish to a live URL, the app must be attached to a Web Hosting plan you own. Buy a plan if none has an open slot, or if you want a separate one. Find one in the catalog, then check out by following `gddy guide shopping`:

```sh
gddy shopping catalog search --category webHosting
```

AI assistants: if an existing plan has an open slot, tell the user and let them choose between attaching the app to it and buying a new plan. Buying is a purchase, so follow the consent steps in `gddy guide shopping` before checkout.

When the order completes, poll `gddy hosting subscription list --hosting-product=WEB_HOSTING` until a plan shows a free slot. Then attach the app to it.

## Publish

```sh
gddy hosting deployment publish --app-id <app-id>
```

If publish returns `WH_PLAN_REQUIRED`, the app has no plan. See *Attach a plan*.

Poll until COMPLETED:

```sh
gddy hosting deployment get --app-id <app-id> --deployment-id <deployment-id>
```

## Custom domains

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

Upload the source again, wait for the import to complete, then publish again. Skip *Create the app* and *Attach a plan*.

## Other commands

| Command | Purpose |
|---|---|
| `hosting app status` | Runtime status for PREVIEW and PUBLISH |
| `hosting app restart --variant <PREVIEW\|PUBLISH>` | Restart an environment |
| `hosting log list` | Fetch log entries (filter by `--variant`, `--level`, `--since`) |
| `hosting secret create/update/delete/list/sync` | Manage per-environment secrets |
| `hosting domain attach/get/detach/list` | Custom domains. Get returns DNS targets for external DNS |
| `hosting runtime get` | View the Node.js runtime version |
| `hosting source github` | Deploy code from a repo already linked in the hosting UI (`source` is GitHub on `app get`) |

## See also

- `gddy guide auth` — how authentication and credential storage work.
