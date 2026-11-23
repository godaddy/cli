//! `api graphql` — inspect and call individual GraphQL operations.
//!
//! Some API domains (e.g. `taxes`, `catalog-products`) are backed by
//! GraphQL instead of OpenAPI specifications. `api operation list --domain
//! <domain>` and `api search` tag each of their addressable query/mutation
//! fields with a `kind`. This group is their own dedicated command surface: `api
//! operation get`/`api parameter list`/`api call` redirect here for a
//! GraphQL operation id, since their REST-shaped output doesn't fit a
//! GraphQL operation at all (see `super::catalog::graphql_operation_redirect_error`).

use cli_engine::{GroupSpec, RuntimeGroupSpec};

mod call;
mod get;
mod sdl;
mod type_cmd;

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("graphql", "Inspect and call individual GraphQL operations").with_long(
            "Some GoDaddy APIs are backed by GraphQL instead of OpenAPI \
            specifications. `api operation list --domain <domain>` and \
            `api search` list each of their queries and mutations as its own \
            operation, with a `kind` of `query` or `mutation`. Use \
            `api graphql get <id>` to show an operation's details and \
            `api graphql call <id>` to run it. `api operation get` and \
            `api call` send you here when you give them a GraphQL operation \
            ID, because their output doesn't fit a GraphQL operation.",
        ),
    )
    .with_command(get::command())
    .with_command(call::command())
    .with_group(type_cmd::group())
    .with_group(sdl::group())
}
