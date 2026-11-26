//! The product commands that change something, run through the whole command
//! tree with `--dry-run`: what the engine does before and after the handler
//! matters here, since the engine's own answer to a preview is "would execute"
//! without running the handler at all.

use std::sync::Arc;

use cli_engine::{Cli, CliConfig, Stage};
use serde_json::{Value, from_str};

use crate::{
    auth::GoDaddyAuthProvider,
    commerce::module,
    environments::{
        self,
        test_support::{ENV_LOCK, EnvGuard},
    },
};

const PRODUCT_ID: &str = "0c3b1a52-77de-4c0e-8f6b-1d2a9e4f5b60";

/// Like [`run`], for output a person reads.
async fn run_human(args: &[&str]) -> (i32, String) {
    let cli = test_cli();
    let output = cli.run(args.iter().copied()).await;
    (output.exit_code, output.rendered)
}

/// The commerce commands as the binary builds them, with its own sign-in and a
/// token supplied the way a script supplies one.
fn test_cli() -> Cli {
    Cli::new(
        CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
            .with_min_stage(Stage::Beta)
            .with_default_auth_provider("godaddy")
            .with_auth_provider(Arc::new(GoDaddyAuthProvider::new()))
            .with_environments(Arc::clone(environments::instance()))
            .with_startup_args(Vec::<&str>::new())
            .with_module(module()),
    )
}

/// Runs `args` and parses the JSON it prints. The token is never sent anywhere:
/// every case that uses this is refused before a request is made.
async fn run(args: &[&str]) -> (i32, Value) {
    let cli = test_cli();
    let output = cli.run(args.iter().copied()).await;
    let parsed = from_str(&output.rendered);
    assert!(parsed.is_ok(), "not json: {}", output.rendered);
    (
        output.exit_code,
        parsed.expect("checked by the assertion above"),
    )
}

fn error_code(json: &Value) -> &str {
    json["error"]["code"].as_str().unwrap_or_default()
}

/// A preview has to fail where the real run would. These stop at the command's own
/// checks, which come before anything is requested, so the engine's generic
/// "would execute" answer would have let every one of them through.
#[tokio::test]
async fn a_preview_refuses_what_the_real_run_would_refuse() {
    let cases: [(&str, &[&str]); 4] = [
        (
            "update with a malformed id",
            &[
                "commerce",
                "catalog",
                "product",
                "update",
                "not-a-uuid",
                "--label",
                "x",
            ],
        ),
        (
            "update with nothing to change",
            &["commerce", "catalog", "product", "update", PRODUCT_ID],
        ),
        (
            "archive with a malformed id",
            &["commerce", "catalog", "product", "archive", "not-a-uuid"],
        ),
        (
            "archive with a malformed id, cascading",
            &[
                "commerce",
                "catalog",
                "product",
                "archive",
                "nope",
                "--with-variants",
            ],
        ),
    ];

    let _lock = ENV_LOCK.lock().await;
    let _pat = EnvGuard::set("GDDY_PAT", "gd_pat_test_12345678");
    let _store = EnvGuard::unset("GDDY_STORE_ID");
    let _store_env = EnvGuard::unset("GDDY_STORE_ID_OTE");
    for (what, command) in cases {
        let mut args = vec!["gddy"];
        args.extend_from_slice(command);
        args.extend(["--dry-run", "--output", "json"]);

        let (exit, json) = run(&args).await;

        assert_ne!(exit, 0, "{what}: {json}");
        assert_eq!(error_code(&json), "VALIDATION_ERROR", "{what}: {json}");
        assert!(json["data"].is_null(), "{what} claimed success: {json}");
    }
}

/// A preview as a person reads it, through the whole command tree. `create` makes
/// no request in a preview, so this runs without an API: it needs only a token
/// and a store, supplied the way a script supplies them. It is what shows that the
/// view registered on the command prints the preview, and not the blank product
/// it printed before.
#[tokio::test]
async fn a_create_preview_is_readable_as_text() {
    let _lock = ENV_LOCK.lock().await;
    let _pat = EnvGuard::set("GDDY_PAT", "gd_pat_test_12345678");
    let _store = EnvGuard::set("GDDY_STORE_ID", "6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b");

    let (exit, text) = run_human(&[
        "gddy",
        "commerce",
        "catalog",
        "product",
        "create",
        "--label",
        "Classic Tee",
        "--type",
        "physical",
        "--dry-run",
        "--output",
        "human",
    ])
    .await;

    assert_eq!(exit, 0, "{text}");
    for wanted in [
        "Action: dry-run: would create product",
        "Name: Classic Tee",
        "Type: PHYSICAL",
    ] {
        assert!(text.contains(wanted), "{wanted} missing: {text}");
    }
    for blank in ["ID:", "Handle:", "Price from", "Created:"] {
        assert!(!text.contains(blank), "{blank} shown: {text}");
    }
}

/// The engine has to leave a preview to the commands, or none of the above can
/// happen: each says it handles `--dry-run` itself and is a mutation.
#[test]
fn each_command_is_a_mutation_that_handles_its_own_preview() {
    use super::{archive, create, update};

    for command in [create::command(), update::command(), archive::command()] {
        let meta = command.spec.metadata();
        assert!(meta.handles_dry_run, "{}", command.spec.name);
        assert!(meta.dry_run_prompt, "{}", command.spec.name);
    }
}

/// A person reads a preview as text, and the product view that a real change uses
/// has none of a preview's fields: it printed every heading of a product with
/// nothing after it, and hid what would happen. These go through the engine's own
/// human renderer with each command's registered view.
mod human {
    use cli_engine::preview_human_view;
    use serde_json::json;

    use super::super::{archive, create, result::change_columns, update};
    use super::*;

    fn rendered(data: Value) -> String {
        preview_human_view(data, &change_columns())
    }

    #[test]
    fn a_preview_shows_what_would_happen_and_to_which_product() {
        let text = rendered(json!({
            "action": "dry-run: would update product",
            "productId": PRODUCT_ID,
            "changes": { "label": "New", "status": "ACTIVE" }
        }));

        for wanted in [
            "Action: dry-run: would update product",
            &format!("Product: {PRODUCT_ID}"),
            "Name: New",
            "Status: ACTIVE",
        ] {
            assert!(text.contains(wanted), "{wanted} missing: {text}");
        }
    }

    #[test]
    fn a_preview_does_not_print_a_products_empty_headings() {
        let text = rendered(json!({
            "action": "dry-run: would create product",
            "changes": { "label": "New", "type": "DIGITAL" }
        }));

        for heading in ["ID:", "Price from", "Created:", "Variants:", "Handle:"] {
            assert!(!text.contains(heading), "{heading} shown: {text}");
        }
        assert!(text.contains("Type: DIGITAL"), "{text}");
    }

    #[test]
    fn an_archive_preview_shows_what_happens_to_the_variants() {
        for (said, shown) in [
            (
                "archived with the product",
                "Variants: archived with the product",
            ),
            ("keep their status", "Variants: keep their status"),
        ] {
            let text = rendered(json!({
                "action": "dry-run: would archive product",
                "productId": PRODUCT_ID,
                "changes": { "variants": said }
            }));

            assert!(text.contains(shown), "{shown} missing: {text}");
        }
    }

    /// A change that was made is a product, shown as `get` shows one, and none of a
    /// preview's headings are printed above it.
    #[test]
    fn a_change_that_was_made_is_shown_as_the_product() {
        let text = rendered(json!({
            "id": PRODUCT_ID, "name": "tee", "label": "Tee", "status": "ACTIVE",
            "type": "PHYSICAL", "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-02T00:00:00Z"
        }));

        for wanted in [
            &format!("ID: {PRODUCT_ID}"),
            "Name: Tee",
            "Handle: tee",
            "Status: ACTIVE",
        ] {
            assert!(text.contains(wanted), "{wanted} missing: {text}");
        }
        for heading in ["Action:", "Product:", "Changes:"] {
            assert!(!text.contains(heading), "{heading} shown: {text}");
        }
    }

    /// The columns are what the commands register, not just what the helper
    /// builds, and the schema they advertise names every field they print.
    #[test]
    fn every_command_registers_the_shared_view_and_schema() {
        for command in [create::command(), update::command(), archive::command()] {
            let name = command.spec.name.clone();
            let columns = command.spec.view_columns;
            assert_eq!(
                columns.iter().map(|c| c.field.as_str()).collect::<Vec<_>>(),
                change_columns()
                    .iter()
                    .map(|c| c.field.as_str())
                    .collect::<Vec<_>>(),
                "{name}"
            );
        }
    }
}
