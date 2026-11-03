## WordPress (MHWP) sites

A WordPress site has no source upload, build or deployment. It is created ready to serve and starts behind a Coming Soon page. `--app-type MHWP` appears once WordPress is enabled for your CLI.

```sh
gddy hosting app create --app-type MHWP --name my-site
gddy hosting operation get --operation-id <operation-id>
```

Take the site live, or back to Coming Soon. Both return an operation ID to poll:

```sh
gddy hosting app publish --app-id <app-id>
gddy hosting app unpublish --app-id <app-id>
gddy hosting operation get --operation-id <operation-id>
```

Going live needs a plan with a free published-site slot; the free plan has none. If publishing is refused because of the plan, upgrade the plan in Airo App Builder or unpublish another site first. `hosting deployment publish` does not apply to WordPress sites.

While a change is still running, repeating the same command returns that running operation; asking for the opposite state is refused until it finishes. Poll the operation, then retry.

| Error | Meaning | Next step |
|---|---|---|
| Plan has no free slot | The plan allows no more live sites | Upgrade the plan in Airo App Builder, or `hosting app unpublish` another site |
| Subscription not active | The Airo App Builder subscription is inactive; taking a site offline still works | Reactivate the subscription, then publish again |
| Change still running | A publish or unpublish for this site has not finished, or another change on this subscription is being accepted at this moment; retry shortly | `hosting operation get --operation-id <operation-id>`, then retry |
| Service unavailable | The plan limit could not be read | Wait a moment and run the command again |

