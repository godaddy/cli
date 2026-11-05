---
summary: Create a managed WordPress site, rename or delete it, and take it live or back behind a placeholder page
---

# `gddy hosting` — managed WordPress sites

Managed WordPress on this platform is a WordPress site that lives inside an Airo App Builder subscription, not a hosting plan of its own. The CLI creates the site, tells you its URL, renames or deletes it, reads its PHP and WordPress versions, and switches it between live and a placeholder page. Everything inside the site — pages, posts, themes, plugins, the site title — is edited in WordPress itself, from the WordPress admin, which you reach by signing in to [Airo App Builder](https://airo-builder.godaddy.com/apps) and opening the site.

## Concepts

**Site** — the WordPress app, created with `--app-type MHWP`. Unlike a Node.js app there is no source upload, no build and no deployment, and no PREVIEW environment. The site is provisioned ready to serve, in the United States, and is reachable at `urls.publish` on `app get` once creation completes. The display name you pass as `--name` labels the app in the CLI and in Airo App Builder; it does not become the WordPress site title.

**Placeholder page** — what the site shows instead of its content until it is taken live, and again after it is unpublished.

**Published-site slot** — a live site uses one slot on the plan. The free plan has none.

**Operation** — create, publish and unpublish return an operation ID. Run `hosting operation get` to check the status of the operation until it reaches a `COMPLETED` or `FAILED` state.

## Create the site

```sh
gddy hosting app create --app-type MHWP --name my-site
gddy hosting operation get --operation-id <operation-id>
```

Creation is asynchronous but usually finishes in seconds. Poll until COMPLETED, then take `app.id` from the response and read the site's URL:

```sh
gddy hosting app get --app-id <app-id>
```

## Take it live, or back behind the placeholder

```sh
gddy hosting app publish --app-id <app-id>
gddy hosting app unpublish --app-id <app-id>
gddy hosting operation get --operation-id <operation-id>
```

Both return an operation ID to poll. Unpublishing changes nothing inside the site: the content stays in place and visitors see the placeholder page again.

Going live needs a plan with a free published-site slot. If publishing is refused because of the plan, upgrade the plan in Airo App Builder or unpublish another site first. `hosting deployment publish` does not apply to WordPress sites.

| Error | Meaning | Next step |
|---|---|---|
| Plan has no free slot | The plan allows no more live sites | Upgrade the plan in Airo App Builder, or `hosting app unpublish` another site |
| Subscription not active | The Airo App Builder subscription is inactive; taking a site offline still works | Reactivate the subscription, then publish again |
| Change still running | A publish or unpublish for this site has not finished, or another change on this subscription is being accepted at this moment; retry shortly | `hosting operation get --operation-id <operation-id>`, then retry |
| Service unavailable | The plan limit could not be read | Wait a moment and run the command again |

## Rename the site

```sh
gddy hosting app update --app-id <app-id> --name my-site
```

`--name` is the label in the CLI and in Airo App Builder. The WordPress site title is edited in WordPress admin. Pass `--name` only. `--root-path` does not apply to a WordPress site. If you include it, nothing changes, not even the name. The name is 1–200 characters and cannot contain `<`, `>` or control characters such as tabs and line breaks.

A site that is being deleted cannot be renamed. If the service is temporarily unavailable, wait a few seconds and run the command again.

## Delete the site

```sh
gddy hosting app delete --app-id <app-id>
gddy hosting app get --app-id <app-id>
```

Deletion removes the site and its content. It cannot be undone. The command returns once deletion has started. `hosting app get` shows status DELETING until the site is gone, and then reports it was not found. There is no operation to poll. If the service is temporarily unavailable, wait a few seconds and run the command again.

## Read the PHP and WordPress versions

```sh
gddy hosting runtime get --app-id <app-id>
```

The response lists each runtime with its version. A WordPress site reports PHP and WORDPRESS. An empty list is not an error. It means no version is known yet. An older site may list WORDPRESS only.

## What the CLI does not do for WordPress sites

`hosting app list`, `get`, `status`, `update`, `delete`, `publish`, `unpublish` and `hosting runtime get` are the commands that work. Deployments, restarts, source imports, secrets, logs, custom domains and plan attachment are not part of the WordPress product at all; the subscription is chosen for you when the site is created. WordPress content and settings are never managed from the CLI.

## See also

- `gddy guide hosting` — the other hosting products and the commands they share.
- `gddy guide auth` — how authentication and credential storage work.
