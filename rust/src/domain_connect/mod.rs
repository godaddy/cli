//! `gddy domain-connect` — Domain Connect template management
//!
//! Create and submit DNS templates to the Domain Connect public registry.
//!
//! Commands:
//! - `template init` — Interactive wizard to create a template
//! - `template validate` — Check a template with no side effects
//! - `template submit` — Submit a template; opens a pull request on Domain-Connect/Templates

use cli_engine::{GroupSpec, Module, RuntimeGroupSpec, Stage};

pub(crate) mod template;

pub fn module() -> Module {
    Module::new("Domain Connect", |_ctx| {
        RuntimeGroupSpec::new(
            GroupSpec::new("domain-connect", "Manage Domain Connect templates").with_long(
                "Create and submit DNS template definitions to the Domain Connect \
                     public registry.\n\
                     \n\
                     • template init     — Create a new template interactively\n\
                     • template validate — Check a template (no side effects)\n\
                     • template submit   — Submit a template; opens a pull request",
            ),
        )
        .with_group(template::group())
    })
    .with_feature_flag("domain-connect", Stage::Experimental)
}
