pub mod client;
mod commands;

use cli_engine::{Module, Stage};

pub fn module() -> Module {
    Module::new("GPA", |_ctx| commands::application_group())
        .with_feature_flag("application", Stage::Experimental)
}
