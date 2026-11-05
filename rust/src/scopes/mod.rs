#[macro_use]
mod declare;
mod auth_scope;
mod command_tree;
mod declarations;
#[cfg(test)]
mod tests;

pub(crate) use auth_scope::auth_scopes_command;
pub(crate) use command_tree::{command_path_aliases, command_scopes};
pub use declarations::*;
pub use declare::ScopeInfo;
