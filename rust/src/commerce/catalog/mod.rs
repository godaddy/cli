//! The Commerce Catalog GraphQL subgraph: the client every `gddy commerce
//! catalog` command will use. No command calls it yet, so the module tree is
//! allowed to be unused until the commands that do land; that allowance is
//! removed with them.

#![expect(
    dead_code,
    reason = "the catalog client has no caller until the `commerce catalog` commands land"
)]

mod client;
mod operations;
