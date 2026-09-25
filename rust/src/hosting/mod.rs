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

use cli_engine::{GroupSpec, Module, RuntimeGroupSpec};

pub fn module() -> Module {
    Module::new("Hosting", |ctx| {
        let mhwp = common::mhwp_enabled(&ctx.middleware().flag_policy);
        RuntimeGroupSpec::new(
            GroupSpec::new("hosting", "Manage GoDaddy hosting products").with_long(
                "Work with GoDaddy hosting APIs.\n\
                 \n\
                 • app          — Hosting applications (create, inspect, update, delete, restart)\n\
                 • deployment   — Build and deploy application source\n\
                 • source       — Import source code\n\
                 • secrets      — Application secrets (create, update, delete, list)\n\
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
                 Commands that act on one environment take an `--environment` flag \
                 (PREVIEW for testing or PUBLISH for public); the older `--variant` name still works.",
            ),
        )
        .with_group(app::group(mhwp))
        .with_group(deployment::group())
        .with_group(source::group())
        .with_group(secrets::group())
        .with_group(log::group())
        .with_group(runtime::group())
        .with_group(domain::group())
        .with_group(subscription::group())
        .with_group(operation::group())
    })
    .with_guides_from_markdown([(
        "hosting.md",
        include_bytes!("guides/hosting.md").as_slice(),
    )])
}
