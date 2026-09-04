//! `gddy db` — access an application's database.
//!
//! Currently exposes `db tunnel`, a MySQL-over-WebSocket bridge to an app's
//! agent. The relay itself lives in [`tunnel`]; this module is wiring only.

use cli_engine::{GroupSpec, Module, RuntimeGroupSpec, Stage};

mod tunnel;

/// The `Database` module: the `db` command group.
pub fn module() -> Module {
    Module::new("Database", |_ctx| {
        RuntimeGroupSpec::new(
            GroupSpec::new("db", "Access an application's database").with_long(
                "Database access helpers for GoDaddy-hosted applications.\n\n\
                 `db tunnel` opens a local TCP port and bridges raw MySQL traffic over \
                 an authenticated WebSocket to the application's agent, which dials the \
                 app's configured database. Point any MySQL client at the local port. \
                 End-to-end MySQL authentication and TLS are preserved — the tunnel \
                 forwards bytes only and never injects or inspects credentials.",
            ),
        )
        .with_command(tunnel::command())
    })
    .with_feature_flag("db", Stage::Experimental)
}
