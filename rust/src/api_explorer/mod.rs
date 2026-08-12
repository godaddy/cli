//! `gddy api` — browse the embedded GoDaddy API catalog and make
//! authenticated requests against any endpoint.
use cli_engine::{GroupSpec, Module, RuntimeGroupSpec};

mod call;
mod catalog;
mod domain_cmd;
mod graphql;
mod http;
mod operation;
mod parameter;
mod response;
mod schema;
mod schema_cmd;
mod schema_tree;
mod search;
mod summary;

pub fn module() -> Module {
    Module::new("API", |_ctx| {
        RuntimeGroupSpec::new(
            GroupSpec::new("api", "Explore and call GoDaddy API endpoints").with_long(
                "Browse the GoDaddy API catalog and make authenticated requests \
                     against any endpoint. Use `api domain list` / `api operation list` to \
                     discover available operations, `api operation get` to inspect parameters, \
                     and `api call` to execute a request with automatic OAuth scope handling.",
            ),
        )
        .with_group(
            RuntimeGroupSpec::new(GroupSpec::new("domain", "Browse API domains").with_long(
                "Browse the top-level API domains available in the embedded catalog. \
                     Each domain groups a related set of endpoints under a shared base URL. \
                     Use `api operation list --domain <domain>` to see the endpoints \
                     within a specific domain.",
            ))
            .with_command(domain_cmd::command()),
        )
        .with_group(
            RuntimeGroupSpec::new(
                GroupSpec::new("operation", "Browse and inspect API operations").with_long(
                    "Browse the operations within a single API domain. \
                     Use `api domain list` first to find available domain names, \
                     then `api operation get <operationId>` to inspect full parameter \
                     and schema details for an individual operation.",
                ),
            )
            .with_command(operation::list_command())
            .with_command(operation::get_command()),
        )
        .with_group(
            RuntimeGroupSpec::new(
                GroupSpec::new("parameter", "Inspect an operation's parameters").with_long(
                    "Inspect the parameters of a single operation, scoped by `--operation`. \
                     A request body counts as a parameter here too, under the synthetic name \
                     `body`.",
                ),
            )
            .with_command(parameter::list_command())
            .with_command(parameter::get_command()),
        )
        .with_group(
            RuntimeGroupSpec::new(
                GroupSpec::new("response", "Inspect an operation's responses").with_long(
                    "Inspect the responses of a single operation, scoped by `--operation`.",
                ),
            )
            .with_command(response::list_command())
            .with_command(response::get_command()),
        )
        .with_group(
            RuntimeGroupSpec::new(GroupSpec::new(
                "schema",
                "Inspect a named or inline API schema",
            ))
            .with_command(schema_cmd::get_command()),
        )
        .with_group(graphql::group())
        .with_command(search::command())
        .with_command(call::command())
    })
    .with_guides_from_markdown([(
        "api-explorer.md",
        include_bytes!("guides/api-explorer.md").as_slice(),
    )])
}
