pub mod nodejs;

use cli_engine::{GroupSpec, Module, RuntimeGroupSpec};

pub fn module() -> Module {
    Module::new("Hosting", |_ctx| {
        RuntimeGroupSpec::new(
            GroupSpec::new("hosting", "Manage GoDaddy hosting products").with_long(
                "Work with GoDaddy hosting APIs.\n\
                 \n\
                 • nodejs — Node.js PaaS applications (create, upload, deploy, logs)",
            ),
        )
        .with_group(nodejs::nodejs_group())
    })
}
