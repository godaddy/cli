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

/// Runs `args` through the commerce commands the way the binary does, with its
/// own sign-in and a token supplied the way a script supplies one. The token is
/// never sent anywhere: every case here is refused before a request is made.
async fn run(args: &[&str]) -> (i32, Value) {
    let cli = Cli::new(
        CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
            .with_min_stage(Stage::Beta)
            .with_default_auth_provider("godaddy")
            .with_auth_provider(Arc::new(GoDaddyAuthProvider::new()))
            .with_environments(Arc::clone(environments::instance()))
            .with_startup_args(Vec::<&str>::new())
            .with_module(module()),
    );
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
