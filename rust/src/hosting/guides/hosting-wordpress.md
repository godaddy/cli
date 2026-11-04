## WordPress (MHWP) sites

Managed WordPress on this platform is a WordPress site that lives inside an Airo App Builder subscription, not a hosting plan of its own. The CLI creates the site, tells you its URL, and switches it between live and a placeholder page. Everything inside the site — pages, posts, themes, plugins, the site title — is edited in WordPress itself, from the WordPress admin, which you reach by signing in to Airo App Builder and opening the site. The API returns no WordPress admin password, so there is no CLI route into the admin.

How it differs from a Node.js app: there is no source upload, no build and no deployment, and no PREVIEW environment. The site is provisioned ready to serve, in the United States, and is reachable at `urls.publish` on `app get` from the moment it exists. Until it is taken live it answers with a placeholder page instead of its content. The display name you pass as `--name` labels the app in the CLI and in Airo App Builder; it does not become the WordPress site title.

`--app-type MHWP` appears once WordPress is enabled for your CLI.

### Create the site

```sh
gddy hosting app create --app-type MHWP --name my-site
gddy hosting operation get --operation-id <operation-id>
```

Creation is asynchronous but usually finishes in seconds. Poll until COMPLETED, then take `app.id` from the response and read the site's URL:

```sh
gddy hosting app get --app-id <app-id>
```

### Take it live, or back behind the placeholder

```sh
gddy hosting app publish --app-id <app-id>
gddy hosting app unpublish --app-id <app-id>
gddy hosting operation get --operation-id <operation-id>
```

Both return an operation ID to poll. Unpublishing changes nothing inside the site: the content stays in place and visitors see the placeholder page again.

The operation's `type` is `SET_PUBLISH_STATE` for these two commands and `CREATE_APP` for a create, so one poller tells them apart.

Going live needs a plan with a free published-site slot; the free plan has none. If publishing is refused because of the plan, upgrade the plan in Airo App Builder or unpublish another site first. `hosting deployment publish` does not apply to WordPress sites.

While a change is still running, repeating the same command returns that running operation; asking for the opposite state is refused until it finishes. Poll the operation, then retry.

| Error | Meaning | Next step |
|---|---|---|
| Plan has no free slot | The plan allows no more live sites | Upgrade the plan in Airo App Builder, or `hosting app unpublish` another site |
| Subscription not active | The Airo App Builder subscription is inactive; taking a site offline still works | Reactivate the subscription, then publish again |
| Change still running | A publish or unpublish for this site has not finished, or another change on this subscription is being accepted at this moment; retry shortly | `hosting operation get --operation-id <operation-id>`, then retry |
| Service unavailable | The plan limit could not be read | Wait a moment and run the command again |

### What the CLI does not do for WordPress sites

`hosting app list`, `get`, `status`, `publish` and `unpublish` are the commands that work. Renaming, deleting and reading the runtime version are planned but not served yet. Deployments, restarts, source imports, secrets, logs, custom domains and plan attachment are not part of the WordPress product at all; the subscription is chosen for you when the site is created. WordPress content and settings are never managed from the CLI.

