//! GoDaddy Developer Platform command namespace.
//!
//! This is intentionally a composition-only module: the application, action
//! catalog, and webhook implementations retain their existing handlers and
//! clients, while `platform` provides their unambiguous common CLI path.

use cli_engine::{GroupSpec, Module, RuntimeGroupSpec, Stage};

pub fn module() -> Module {
    Module::new("Platform", |_ctx| {
        RuntimeGroupSpec::new(
            GroupSpec::new("platform", "Build and manage GoDaddy Platform integrations")
                .with_long(
                    "Build integrations for the GoDaddy Developer Platform. Manage your app, \
                     browse action contracts, and inspect webhook event types from one \
                     namespace. Use `gddy platform app init` to create an app.",
                )
                .with_feature_flag("platform", Stage::Experimental),
        )
        .with_group(crate::application::group())
        .with_group(crate::actions_catalog::group())
        .with_group(crate::webhook::group())
    })
}
