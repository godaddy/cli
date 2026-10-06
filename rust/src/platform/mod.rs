//! GoDaddy Developer Platform command namespace.
//!
//! This is intentionally a composition-only module: the application, action
//! catalog, and webhook implementations retain their existing handlers and
//! clients, while `platform` provides their unambiguous common CLI path.

use cli_engine::{GroupSpec, Module, RuntimeGroupSpec};

mod actions;
pub mod app;
mod webhook;

pub fn module() -> Module {
    Module::new("Platform", |_ctx| {
        RuntimeGroupSpec::new(
            GroupSpec::new("platform", "Build and manage GoDaddy Platform integrations").with_long(
                "Build integrations for the GoDaddy Developer Platform. Manage your app, \
                     browse action contracts, and inspect webhook event types from one \
                     namespace. Use `gddy platform app init` to create an app.",
            ),
        )
        .with_group(app::group())
        .with_group(actions::group())
        .with_group(webhook::group())
    })
    .with_guides_from_markdown([
        (
            "platform-overview.md",
            include_bytes!("guides/platform-overview.md").as_slice(),
        ),
        (
            "platform-settings.md",
            include_bytes!("guides/platform-settings.md").as_slice(),
        ),
    ])
}

#[cfg(test)]
mod tests {
    #[test]
    fn platform_overview_guide_does_not_document_native_extension() {
        let guide = include_str!("guides/platform-overview.md");
        assert!(
            !guide.contains("native-extension"),
            "overview guide still documents add native-extension"
        );
        assert!(
            !guide.contains("[native_extension]"),
            "overview guide still documents the native_extension section"
        );
    }
}
