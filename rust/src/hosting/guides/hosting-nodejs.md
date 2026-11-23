---
summary: Create a Node.js app on GoDaddy hosting, upload your code, preview the app, attach it to a plan, and publish it
---

# `gddy hosting`: deploy a Node.js application

This guide covers the full lifecycle of a hosted Node.js application. Create an app, upload source code, test on a staging URL, attach to a hosting plan, and publish to production.

## Concepts

The following terms and concepts are relevant to deploying a Node.js application:

**App**: the hosted Node.js application. Create it first to get the `--app-id` used for later commands.

**Environment**: each app has a PREVIEW (staging) and PUBLISH (production) environment. The CLI calls an environment a `variant`. Uploads land on PREVIEW and then can be promoted to PUBLISH once accepted.

**Subscription**: a hosting plan you pay for. A subscription can host one or more apps. Attach the app to one before the first publish. If you don't have a plan, go to the "Buy a plan" section in this guide.

**Slot**: available space within a subscription for an attached app. Each app attached to a subscription takes up one slot, and a given subscription can have one or more slots.

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

## Upload the source code

Deploys code to the app's PREVIEW environment. You can run this again to replace what was uploaded before.

```sh
gddy hosting source upload --app-id <app-id> --file ./app.zip
```

Poll the import until COMPLETED:

```sh
gddy hosting source status --app-id <app-id> --import-id <import-id>
```

## Preview the app

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

If `totalAvailableSlots` is 0, go to the "Buy a plan" section later in this guide.

Skip this on later deploys.

## Buy a plan

To publish to a live URL, the app must be attached to a Web Hosting plan you own. Buy a plan if none has an open slot, or if you want a separate one. Find one in the catalog, then check out by following `gddy guide shopping`:

```sh
gddy shopping catalog search --category webHosting
```

AI assistants: if an existing plan has an open slot, tell the user and let them choose between attaching the app to it and buying a new plan. Buying is a purchase, so follow the consent steps in `gddy guide shopping` before checkout.

When the order completes, poll `gddy hosting subscription list --hosting-product=WEB_HOSTING` until a plan with a free slot displays whose `subscriptionId` wasn't in the list before checkout. Attach the app to that plan.

## Publish the app

```sh
gddy hosting deployment publish --app-id <app-id>
```

If publish returns `WH_PLAN_REQUIRED`, the app has no plan. Go to the "Attach a plan" section earlier in this guide.

Poll until COMPLETED:

```sh
gddy hosting deployment get --app-id <app-id> --deployment-id <deployment-id>
```

## Custom domains

Every domain on an app has a `domainType`:

- **`PREFIX`**: the hostname GoDaddy gives the app. By default it starts with the app's ID. A `PREFIX` domain replaces that first part with a name you choose, for example `helloworld.c39.airoapp.ai`. You change the prefix in the hosting UI, not with this CLI.
- **`CUSTOM`**: a hostname you own and attach to the app. If its DNS is on GoDaddy, you don't need to do anything more.

Attach a `CUSTOM` domain after the app is published. Then read it back with all fields, because `domainType` isn't in the default output of `domain get` or `domain list`:

```sh
gddy hosting domain attach --app-id <app-id> --hostname www.example.com
gddy hosting domain get --app-id <app-id> --domain-id <domain-id> --fields all
```

The following examples show the `data` part of the output. The CLI also prints `next_actions`, which suggests the command to detach the domain. This is a `PREFIX` domain:

```json
{
  "domainId": "aGVsbG93b3JsZC5jMzkuYWlyb2FwcC5haQ",
  "domainType": "PREFIX",
  "hostname": "helloworld.c39.airoapp.ai",
  "role": "PRIMARY",
  "verificationStatus": "ACTIVE"
}
```

This is a `CUSTOM` domain that isn't verified yet:

```json
{
  "domainId": "d3d3LmV4YW1wbGUuY29t",
  "domainType": "CUSTOM",
  "hostname": "www.example.com",
  "role": "PRIMARY",
  "verificationStatus": "PENDING"
}
```

For a CUSTOM domain whose DNS isn't on GoDaddy, poll `hosting domain get` and apply records at the external DNS host as fields become non-null:

| When | Record | Target |
|---|---|---|
| `certificateValidationCname` is set | CNAME `_acme-challenge` | that hostname (proves ownership / issues TLS) |
| `anycastIp` is set | A for the attached hostname | that IPv4 address (traffic to hosting) |

Keep polling until `verificationStatus` is `ACTIVE`. `cdnStatus` follows CDN provisioning (`INIT` → `PENDING` → `ACTIVE`).

## Redeploy the app

Upload the source again, wait for the import to complete, then publish again. Skip the "Create the app" and "Attach a plan" sections.

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

- `gddy guide hosting`: the other hosting products and the commands they share.
- `gddy guide auth`: how authentication and credential storage work.
