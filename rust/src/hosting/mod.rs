mod app;
pub mod client;
pub mod common;
mod deployment;
mod domain;
mod log;
mod operation;
mod runtime;
mod secrets;
mod source;
mod subscription;
#[cfg(test)]
mod test_support;

use cli_engine::{GroupSpec, Module, RuntimeGroupSpec};

pub fn module() -> Module {
    Module::new("Hosting", |_ctx| {
        RuntimeGroupSpec::new(
            GroupSpec::new("hosting", "Manage GoDaddy hosting products").with_long(
                "Work with GoDaddy hosting APIs.\n\
                 \n\
                 • app          — Hosting applications (create, inspect, update, delete, restart, publish/unpublish)\n\
                 • deployment   — Build and deploy application source\n\
                 • source       — Import source code\n\
                 • secret       — Application secrets (create, update, delete, list)\n\
                 • log          — Application log entries\n\
                 • runtime      — Application runtime configuration\n\
                 • domain       — Domains attached to an application\n\
                 • subscription — Hosting plan subscriptions\n\
                 • operation    — Poll async operations\n\
                 \n\
                 First-time setup (Node.js):\n\
                 1. `hosting app create` — provision the app, poll `hosting operation get`\n\
                 2. `hosting source upload` — upload source, poll `hosting source status`\n\
                 3. Preview at the PREVIEW URL from `hosting app get` (no plan needed)\n\
                 4. `hosting subscription attach` — one-time; required before publish\n\
                 5. `hosting deployment publish` — deploy to PUBLISH, poll `hosting deployment get`\n\
                 \n\
                 To redeploy: repeat steps 2–3, then 5.\n\
                 \n\
                 WordPress sites (`--app-type MHWP`) have no source, build or deployment: \
                 see `gddy guide hosting-wordpress`.\n\
                 \n\
                 Terminology: `variant` refers to an environment — PREVIEW (staging) or PUBLISH (production).",
            ),
        )
        .with_group(app::group())
        .with_group(deployment::group())
        .with_group(source::group())
        .with_group(secrets::group())
        .with_group(log::group())
        .with_group(runtime::group())
        .with_group(domain::group())
        .with_group(subscription::group())
        .with_group(operation::group())
    })
    // One call per guide: a call sorts its files by name, which would list
    // `hosting-nodejs` ahead of the `hosting` index.
    .with_guides_from_markdown([("hosting.md", common::HOSTING_GUIDE)])
    .with_guides_from_markdown([("hosting-nodejs.md", common::NODEJS_GUIDE)])
    .with_guides_from_markdown([("hosting-wordpress.md", common::WORDPRESS_GUIDE)])
}

#[cfg(test)]
mod tests {
    use super::common::{HOSTING_GUIDE, NODEJS_GUIDE, WORDPRESS_GUIDE};
    use crate::hosting::test_support::{cli, run_guide};

    #[test]
    fn the_index_lists_each_product_and_the_publish_command_once() {
        for added in [
            "`gddy guide hosting-nodejs`",
            "`gddy guide hosting-wordpress`",
            "`hosting app publish --app-id <app-id>`",
        ] {
            assert_eq!(
                HOSTING_GUIDE.matches(added).count(),
                1,
                "{added}: {HOSTING_GUIDE}"
            );
        }
        let position = |text: &str| HOSTING_GUIDE.find(text).expect(text);
        assert!(
            position("hosting-nodejs") < position("hosting-wordpress"),
            "the WordPress row must follow the Node.js row: {HOSTING_GUIDE}"
        );
        assert!(
            position("hosting app status") < position("hosting app publish"),
            "the publish row must follow the status row: {HOSTING_GUIDE}"
        );
    }

    #[test]
    fn the_nodejs_guide_never_mentions_wordpress() {
        for absent in [
            "WordPress",
            "MHWP",
            "hosting app publish",
            "placeholder page",
        ] {
            assert!(!NODEJS_GUIDE.contains(absent), "mentions {absent:?}");
        }
    }

    /// The walkthrough has to say what the product is, not only which commands
    /// to type: a reader who has never seen it needs to know where the content
    /// is edited and how to get there, and that there is no build or deploy.
    #[test]
    fn the_wordpress_guide_explains_the_product() {
        for present in [
            "Airo App Builder subscription",
            "WordPress admin",
            "(https://airo-builder.godaddy.com/apps)",
            "## Concepts",
            "no source upload, no build",
            "does not become the WordPress site title",
            "placeholder page",
            "## What the CLI does not do for WordPress sites",
            "hosting app update",
            "hosting app delete",
            "There is no operation to poll. If the service is temporarily unavailable, wait a \
             few seconds and run the command again. To take the site offline but keep its \
             content, run `hosting app unpublish` instead.",
            "hosting runtime get",
            "Deployments, restarts, source imports, secrets, logs, custom domains and plan attachment",
        ] {
            assert!(
                WORDPRESS_GUIDE.contains(present),
                "the guide is missing {present:?}"
            );
        }
        assert!(
            !WORDPRESS_GUIDE.contains("planned but not served"),
            "{WORDPRESS_GUIDE}"
        );
    }

    #[tokio::test]
    async fn gddy_guide_offers_the_index_then_each_product() {
        let cli = cli();
        let topics = run_guide(&cli, None).await;
        // Listed index first, then the product guides, by their summaries.
        let position = |summary: &str| topics.find(summary).expect(summary);
        assert!(
            position("Start here for GoDaddy hosting") < position("Create a Node.js app")
                && position("Create a Node.js app") < position("Create a managed WordPress site"),
            "{topics}"
        );
        let index = run_guide(&cli, Some("hosting")).await;
        assert!(index.contains("gddy guide hosting-wordpress"), "{index}");
        let nodejs = run_guide(&cli, Some("hosting-nodejs")).await;
        assert!(nodejs.contains("Attach a plan"), "{nodejs}");
        let wordpress = run_guide(&cli, Some("hosting-wordpress")).await;
        assert!(wordpress.contains("hosting app unpublish"), "{wordpress}");
    }
}
