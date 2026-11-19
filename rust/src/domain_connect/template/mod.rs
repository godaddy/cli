//! Template subcommands: init, validate and submit

use cli_engine::{GroupSpec, RuntimeGroupSpec};

mod common;
mod init;
mod interactive;
mod parsers;
mod submit;
mod types;
mod validate;

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new(
        "template",
        "Create, check and submit Domain Connect templates",
    ))
    .with_command(init::command())
    .with_command(validate::command())
    .with_command(submit::command())
}
