pub mod nodejs;

mod app;
pub mod client;
pub mod common;
mod deployment;
mod domain;
mod github;
mod log;
mod operation;
mod runtime;
mod secrets;
mod source;
mod subscription;

use cli_engine::{GroupSpec, Module, RuntimeGroupSpec, Stage};

pub fn module() -> Module {
    Module::new("Hosting", |_ctx| {
        RuntimeGroupSpec::new(
            GroupSpec::new("hosting", "Manage GoDaddy hosting products").with_long(
                "Work with GoDaddy hosting APIs.\n\
                 \n\
                 • app          — Hosting applications (create, inspect, update, delete, restart)\n\
                 • deployment   — Build and deploy application source\n\
                 • source       — Import source code\n\
                 • github       — GitHub connection, repositories, branches\n\
                 • secrets      — Application secrets (create, update, delete, list)\n\
                 • log          — Application log entries\n\
                 • runtime      — Application runtime configuration\n\
                 • domain       — Domains attached to an application\n\
                 • subscription — Hosting plan subscriptions\n\
                 • operation    — Poll async operations\n\
                 • nodejs       — Node.js PaaS applications (legacy; use `app` for new work)",
            ),
        )
        .with_group(app::group())
        .with_group(deployment::group())
        .with_group(source::group())
        .with_group(github::group())
        .with_group(secrets::group())
        .with_group(log::group())
        .with_group(runtime::group())
        .with_group(domain::group())
        .with_group(subscription::group())
        .with_group(operation::group())
        .with_group({
            let mut g = nodejs::nodejs_group();
            g.group = g.group.hidden(true);
            g
        })
    })
    .with_feature_flag("hosting", Stage::Beta)
}
