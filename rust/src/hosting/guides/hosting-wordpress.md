---
summary: Create a managed WordPress site, rename or delete it, and take it live or back behind a placeholder page
---

# `gddy hosting`: managed WordPress sites

Managed WordPress on this platform is a WordPress site that lives inside an Airo App Builder subscription, not a hosting plan of its own. The CLI creates the site, tells you its URL, renames or deletes it, reads its PHP and WordPress versions, and switches it between live and a placeholder page. You edit everything inside the site in WordPress itself: pages, posts, themes, plugins and the site title. Open the WordPress admin by signing in to [Airo App Builder](https://airo-builder.godaddy.com/apps) and opening the site.

## Concepts

The following terms and concepts are relevant to managed WordPress sites:

**Site**: the WordPress app, created with `--app-type MHWP`. Unlike a Node.js app there is no source upload, no build and no deployment, and no PREVIEW environment. The site is provisioned ready to serve, in the United States, and is reachable at `urls.publish` on `app get` once creation completes. The display name you pass as `--name` labels the app in the CLI and in Airo App Builder; it doesn't become the WordPress site title.

**Placeholder page**: what the site shows instead of its content until it's taken live, and again after it's unpublished.

**Published-site slot**: a live site uses one slot on the plan. The free plan has none.

**Operation**: create, publish and unpublish return an operation ID. Run `hosting operation get` to check the status of the operation until it reaches a `COMPLETED` or `FAILED` state.

## Create the site

```sh
gddy hosting app create --app-type MHWP --name my-site
gddy hosting operation get --operation-id <operation-id>
```

If `app create` fails or times out, follow the next step in the error. When it tells you to run the same command again with the same `--name` before a given time, you get the first request's result, not a second site. This holds only if you run it on the same computer before that time. On another computer, or later than that, first run `gddy hosting app list --app-type MHWP` and create the site again only if it isn't there.

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

Going live needs a plan with a free published-site slot. If publishing is refused because of the plan, upgrade the plan in Airo App Builder or unpublish another site first. `hosting deployment publish` doesn't apply to WordPress sites.

| Error | Meaning | Next step |
|---|---|---|
| Plan has no free slot | The plan allows no more live sites | Upgrade the plan in Airo App Builder, or `hosting app unpublish` another site |
| Subscription not active | The Airo App Builder subscription is inactive; taking a site offline still works | Reactivate the subscription, then publish again |
| Change still running | A publish or unpublish for this site hasn't finished, or another change on this subscription is being accepted at this moment; retry shortly | `hosting operation get --operation-id <operation-id>`, then retry |
| Service unavailable | The plan limit couldn't be read | Wait a moment and run the command again |

## Rename the site

```sh
gddy hosting app update --app-id <app-id> --name my-site
```

`--name` is the label in the CLI and in Airo App Builder. The WordPress site title is edited in WordPress admin. Pass `--name` only. `--root-path` doesn't apply to a WordPress site. If you include it, nothing changes, not even the name. The name is 1–200 characters and can't contain `<`, `>` or control characters, including tabs and line breaks.

A site that's being deleted can't be renamed. If the service is temporarily unavailable, wait a few seconds and run the command again.

## Delete the site

```sh
gddy hosting app delete --app-id <app-id>
gddy hosting app get --app-id <app-id>
```

Deletion removes the site and its content. It can't be undone. The command returns once deletion has started. `hosting app get` shows status DELETING until the site is gone, and then reports it wasn't found. There is no operation to poll. If the service is temporarily unavailable, wait a few seconds and run the command again.

## Read the PHP and WordPress versions

```sh
gddy hosting runtime get --app-id <app-id>
```

The response lists each runtime with its version. A WordPress site reports PHP and WORDPRESS. An empty list isn't an error. It means no version is known yet. An older site may list WORDPRESS only.

## What the CLI doesn't do for WordPress sites

`hosting app list`, `get`, `status`, `update`, `delete`, `publish`, `unpublish` and `hosting runtime get` are the commands that work. Deployments, restarts, source imports, secrets, logs, custom domains and plan attachment aren't part of the WordPress product at all. The platform chooses the subscription for you when it creates the site. WordPress content and settings are never managed from the CLI.

## See also

- `gddy guide hosting`: the other hosting products and the commands they share.
- `gddy guide auth`: how authentication and credential storage work.
