mod attach;
mod detach;
mod get;
mod list;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("domain", "Manage domains attached to an application").with_long(
            "List, inspect, attach, and detach domains for a hosting application. \
             PREFIX hostnames need no customer DNS. CUSTOM hostnames whose DNS is \
             outside GoDaddy need `_acme-challenge` CNAME (certificateValidationCname) \
             and an A record (anycastIp); poll `hosting domain get` until ACTIVE.",
        ),
    )
    .with_command(list::command())
    .with_command(get::command())
    .with_command(attach::command())
    .with_command(detach::command())
}
