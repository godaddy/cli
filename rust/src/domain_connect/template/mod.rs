//! Template subcommands: init, validate and submit

use cli_engine::{GroupSpec, RuntimeGroupSpec};

mod client;
mod common;
mod init;
mod interactive;
mod parsers;
mod submit;
mod types;
mod validate;

pub(crate) use types::*;

pub fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new(
        "template",
        "Domain Connect template commands",
    ))
    .with_command(init::command())
    .with_command(validate::command())
    .with_command(submit::command())
}
