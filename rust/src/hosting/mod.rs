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
    Module::new("Hosting", |ctx| {
        let mhwp = common::mhwp_enabled(&ctx.middleware().flag_policy);
        // Registered here, not on the Module, so the guides are built from the
        // same flag that decides which commands exist: they must never walk a
        // reader into `hosting app publish` on a binary that does not register it.
        // One call per guide: a call sorts its files by name, which would list
        // `hosting-nodejs` ahead of the `hosting` index.
        ctx.add_guides_from_markdown([("hosting.md", common::hosting_guide(mhwp))]);
        ctx.add_guides_from_markdown([("hosting-nodejs.md", common::NODEJS_GUIDE)]);
        if mhwp {
            ctx.add_guides_from_markdown([("hosting-wordpress.md", common::WORDPRESS_GUIDE)]);
        }
        // Only WordPress sites have a publish state; name it only while the flag is on.
        let app_line = if mhwp {
            "Hosting applications (create, inspect, update, delete, restart, publish/unpublish)"
        } else {
            "Hosting applications (create, inspect, update, delete, restart)"
        };
        RuntimeGroupSpec::new(
            GroupSpec::new("hosting", "Manage GoDaddy hosting products").with_long(format!(
                "Work with GoDaddy hosting APIs.\n\
                 \n\
                 • app          — {app_line}\n\
                 • deployment   — Build and deploy application source\n\
                 • source       — Import source code\n\
                 • secret       — Application secrets (create, update, delete, list)\n\
                 • log          — Application log entries\n\
                 • runtime      — Application runtime configuration\n\
                 • domain       — Domains attached to an application\n\
                 • subscription — Hosting plan subscriptions\n\
                 • operation    — Poll async operations\n\
                 \n\
                 First-time setup:\n\
                 1. `hosting app create` — provision the app, poll `hosting operation get`\n\
                 2. `hosting source upload` — upload source, poll `hosting source status`\n\
                 3. Preview at the PREVIEW URL from `hosting app get` (no plan needed)\n\
                 4. `hosting subscription attach` — one-time; required before publish\n\
                 5. `hosting deployment publish` — deploy to PUBLISH, poll `hosting deployment get`\n\
                 \n\
                 To redeploy: repeat steps 2–3, then 5.\n\
                 \n\
                 Terminology: `variant` refers to an environment — PREVIEW (staging) or PUBLISH (production)."
            )),
        )
        .with_group(app::group(mhwp))
        .with_group(deployment::group())
        .with_group(source::group())
        .with_group(secrets::group())
        .with_group(log::group())
        .with_group(runtime::group())
        .with_group(domain::group())
        .with_group(subscription::group())
        .with_group(operation::group(mhwp))
    })
}

#[cfg(test)]
mod tests {
    use super::common::{NODEJS_GUIDE, WORDPRESS_GUIDE, hosting_guide};
    use crate::hosting::test_support::{cli, run_guide};

    #[test]
    fn the_index_names_wordpress_only_while_its_flag_is_on() {
        let off = hosting_guide(false);
        assert_eq!(off, include_str!("guides/hosting.md"));
        assert!(off.contains("`gddy guide hosting-nodejs`"), "{off}");
        for absent in ["WordPress", "MHWP", "hosting-wordpress", "app publish"] {
            assert!(!off.contains(absent), "flag off still mentions {absent:?}");
        }

        let on = hosting_guide(true);
        for added in [
            "`gddy guide hosting-wordpress`",
            "`hosting app publish --app-id <app-id>`",
        ] {
            assert_eq!(on.matches(added).count(), 1, "{added}: {on}");
        }
        assert!(
            on.find("hosting-nodejs") < on.find("hosting-wordpress"),
            "the WordPress row must follow the Node.js row: {on}"
        );
        assert!(
            on.find("hosting app status") < on.find("hosting app publish"),
            "the publish row must follow the status row: {on}"
        );
        // The two rows are the only difference.
        let without_rows: Vec<&str> = on
            .lines()
            .filter(|line| !line.contains("hosting-wordpress") && !line.contains("app publish"))
            .collect();
        assert_eq!(without_rows, off.lines().collect::<Vec<_>>());
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
    /// is edited, that there is no build or deploy, and that the CLI does not
    /// hold the WordPress password.
    #[test]
    fn the_wordpress_guide_explains_the_product() {
        for present in [
            "Airo App Builder subscription",
            "WordPress admin",
            "## Concepts",
            "no source upload, no build",
            "does not become the WordPress site title",
            "placeholder page",
            "## What the CLI does not do for WordPress sites",
        ] {
            assert!(
                WORDPRESS_GUIDE.contains(present),
                "the guide is missing {present:?}"
            );
        }
    }

    #[tokio::test]
    async fn gddy_guide_offers_wordpress_only_while_its_flag_is_on() {
        let off = cli(false);
        let topics = run_guide(&off, None).await;
        assert!(topics.contains("hosting-nodejs"), "{topics}");
        assert!(!topics.contains("hosting-wordpress"), "{topics}");
        let index = run_guide(&off, Some("hosting")).await;
        assert!(!index.contains("WordPress"), "{index}");
        let nodejs = run_guide(&off, Some("hosting-nodejs")).await;
        assert!(nodejs.contains("Attach a plan"), "{nodejs}");
        let wordpress = run_guide(&off, Some("hosting-wordpress")).await;
        assert!(!wordpress.contains("Airo App Builder"), "{wordpress}");

        let on = cli(true);
        let topics = run_guide(&on, None).await;
        // Listed index first, then the product guides, by their summaries.
        let position = |summary: &str| topics.find(summary).expect(summary);
        assert!(
            position("Start here for GoDaddy hosting") < position("Deploy a Node.js app")
                && position("Deploy a Node.js app") < position("Create a managed WordPress site"),
            "{topics}"
        );
        let index = run_guide(&on, Some("hosting")).await;
        assert!(index.contains("gddy guide hosting-wordpress"), "{index}");
        let wordpress = run_guide(&on, Some("hosting-wordpress")).await;
        assert!(wordpress.contains("hosting app unpublish"), "{wordpress}");
    }
}
