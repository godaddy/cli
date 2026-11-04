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
        // Registered here, not on the Module, so the guide is built from the same
        // flag that decides which commands exist: it must never walk a reader
        // into `hosting app publish` on a binary that does not register it.
        ctx.add_guides_from_markdown([("hosting.md", common::hosting_guide(mhwp))]);
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
    use super::common::hosting_guide;
    use crate::hosting::test_support::{cli, run_hosting_guide};

    #[test]
    fn the_guide_is_the_node_js_guide_until_the_wordpress_flag_is_on() {
        let off = hosting_guide(false);
        assert_eq!(off, include_str!("guides/hosting.md"));
        for absent in [
            "WordPress",
            "MHWP",
            "app publish",
            "app unpublish",
            "placeholder page",
        ] {
            assert!(!off.contains(absent), "flag off still mentions {absent:?}");
        }

        let on = hosting_guide(true);
        for present in [
            "## WordPress (MHWP) sites",
            "gddy hosting app publish --app-id",
            "gddy hosting app unpublish --app-id",
            "| `hosting app publish` / `hosting app unpublish` |",
            "Restart an environment (Node.js) |",
        ] {
            assert!(on.contains(present), "flag on is missing {present:?}");
        }
        // Spliced in once, before Other commands, and nothing was dropped.
        assert_eq!(on.matches("## WordPress (MHWP) sites").count(), 1);
        assert_eq!(on.matches("## Other commands").count(), 1);
        assert!(
            on.find("## WordPress (MHWP) sites") < on.find("## Other commands"),
            "the WordPress section must precede Other commands"
        );
        assert!(!on.contains("Restart an environment |"), "{on}");
        // Undoing both splices gives the flag-off guide back, so the WordPress
        // text is purely additive.
        let undone = on
            .replace(include_str!("guides/hosting-wordpress.md"), "")
            .replace(
                "Restart an environment (Node.js) |\n| `hosting app publish` / \
                 `hosting app unpublish` | Take a WordPress site live, or back to a \
                 placeholder page |",
                "Restart an environment |",
            );
        assert_eq!(undone, off);
    }

    /// The walkthrough has to say what the product is, not only which commands
    /// to type: a reader who has never seen it needs to know where the content
    /// is edited, that there is no build or deploy, and that the CLI does not
    /// hold the WordPress password.
    #[test]
    fn the_wordpress_section_explains_the_product() {
        let on = hosting_guide(true);
        for present in [
            "Airo App Builder subscription",
            "WordPress admin",
            "no source upload, no build",
            "does not become the WordPress site title",
            "placeholder page",
            "### What the CLI does not do for WordPress sites",
        ] {
            assert!(on.contains(present), "the guide is missing {present:?}");
        }
    }

    #[tokio::test]
    async fn gddy_guide_hosting_follows_the_wordpress_flag() {
        let off = run_hosting_guide(&cli(false)).await;
        assert!(!off.contains("unpublish"), "{off}");
        assert!(!off.contains("WordPress"), "{off}");
        assert!(off.contains("Attach a plan"), "{off}");

        let on = run_hosting_guide(&cli(true)).await;
        assert!(on.contains("WordPress (MHWP) sites"), "{on}");
        assert!(on.contains("hosting app unpublish"), "{on}");
    }
}
