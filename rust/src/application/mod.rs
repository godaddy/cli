pub mod client;
mod commands;

use cli_engine::Module;

pub fn module() -> Module {
    Module::new("GPA", |_ctx| commands::application_group())
}
