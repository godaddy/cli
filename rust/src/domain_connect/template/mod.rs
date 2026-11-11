//! Template subcommands: init and submit

use cli_engine::{GroupSpec, RuntimeGroupSpec};

mod init;
mod submit;
mod types;

pub(crate) use types::*;

pub fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new(
        "template",
        "Domain Connect template commands",
    ))
    .with_command(init::command())
    .with_command(submit::command())
}
