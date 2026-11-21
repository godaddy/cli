//! `gddy commerce` — merchant-facing commands over the Commerce Catalog and
//! Stores APIs.
//!
//! Only the store-context layer exists so far. It is not reachable from any
//! command yet, so the module tree is allowed to be unused until the commands
//! that call it land; that allowance is removed with them.

#![expect(
    dead_code,
    reason = "the store context has no caller until the `commerce store` commands land"
)]

pub(crate) mod store;
