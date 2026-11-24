use std::path::Path;

use chrono::{DateTime, Utc};
use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier,
};
use serde_json::{Value, json};

use super::create_keys;
use crate::error::GddyError;
use crate::hosting::client::{ClientError, HostingClient};
use crate::hosting::common::{
    AppHostingProduct, CREATE_APP, HostingAppOperation, HostingAppType, client_err,
    client_err_with_fix, error_issues, make_client, with_operation_type,
};
use crate::next_action::next_action;
use crate::scopes::HOSTING_APPLICATION_CREATE as APP_CREATE;

#[derive(Debug, Clone, clap::Args)]
struct AppCreateArgs {
    /// Application type (required).
    #[arg(long = "app-type", value_name = "TYPE", ignore_case = true)]
    app_type: HostingAppType,

    /// Human-readable display name (1–200 characters).
    #[arg(long, value_name = "NAME")]
    name: String,

    /// What hosts a new Node.js app: WEB_HOSTING (a Web Hosting plan) or AIRO
    /// (your Airo App Builder plan). Omit it to get WEB_HOSTING if you have a
    /// Web Hosting plan. Only for --app-type NODEJS.
    #[arg(long, value_name = "PRODUCT", ignore_case = true)]
    hosting_product: Option<AppHostingProduct>,
}

impl AppCreateArgs {
    /// The create these arguments ask for. A hosting product is a choice only
    /// a Node.js app has, so it is refused for any other type here, before
    /// the create is sent.
    fn create(&self) -> Result<create_keys::Create<'_>, GddyError> {
        if self.hosting_product.is_some() && self.app_type != HostingAppType::Nodejs {
            return Err(GddyError::validation(format!(
                "--hosting-product applies to --app-type NODEJS only, not {}",
                self.app_type.as_str()
            ))
            .with_fix("Run the command again without --hosting-product.")
            .with_system("hosting"));
        }
        Ok(create_keys::Create {
            app_type: self.app_type,
            hosting_product: self.hosting_product,
            name: &self.name,
        })
    }
}

const NODEJS_HELP: &str = "\n\nA Node.js app is hosted on a Web Hosting plan or on your Airo \
     App Builder plan. `--hosting-product` chooses which one, and `hosting app get` shows it \
     as `hostingProduct`. Without the flag, you get a Web Hosting app if you have a Web \
     Hosting plan; otherwise the choice depends on your account. Only a WEB_HOSTING app \
     needs a plan attached before it goes live. An AIRO app has no plan to attach.";

const MHWP_HELP: &str = "\n\nMHWP creates a WordPress site and needs an Airo App Builder \
     subscription. The site starts behind a placeholder page, and its WordPress title is \
     not set from --name. It is hosted in the US. Log in to WordPress admin from Airo App \
     Builder. Take the site live with `hosting app publish` (needs a plan with a free \
     published-site slot) and back behind the placeholder with `hosting app unpublish`. \
     For MHWP apps, these commands work: `hosting app get`, `status`, `update`, \
     `delete`, `publish`, `unpublish` and `hosting runtime get`. Deployments, restarts, \
     source imports, secrets, logs, custom domains and plan attachment are not available \
     for WordPress sites.";

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppCreateArgs, _, _, _>(
        CommandSpec::from_args::<AppCreateArgs>("create", "Create a hosting application")
            .with_long(format!(
                "Provision a new hosting application slot. Because no app ID exists \
                 until provisioning completes, this returns an operation ID. \
                 Poll `hosting operation get --operation-id <id>` until \
                 status is COMPLETED or FAILED. On COMPLETED, the operation's \
                 `app` field carries the created app; use `app.id` \
                 as the --app-id for all subsequent calls.{NODEJS_HELP}{MHWP_HELP}"
            ))
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[APP_CREATE])
            .with_output_schema::<HostingAppOperation>(),
        |ctx, args: AppCreateArgs| async move {
            let create = args.create().map_err(GddyError::into_cli_error)?;
            let app_type = create.app_type;
            let client = make_client(&ctx, &[APP_CREATE]).await?;
            let data = submit(&client, create_keys::path().as_deref(), create).await?;
            let data = with_operation_type(data, CREATE_APP);

            let operation_id = data
                .get("operationId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();

            let poll_description = poll_description(app_type);
            let mut poll_action = next_action(
                "hosting operation get --operation-id <operation-id>",
                poll_description,
            )
            .with_param("operation-id", NextActionParam::required());

            if !operation_id.is_empty() {
                poll_action = next_action(
                    "hosting operation get --operation-id <operation-id>",
                    poll_description,
                )
                .with_param("operation-id", NextActionParam::value(operation_id));
            }

            Ok(CommandResult::new(data).with_next_actions(vec![poll_action]))
        },
    )
}

fn poll_description(app_type: HostingAppType) -> &'static str {
    match app_type {
        HostingAppType::Nodejs => "Poll app provisioning status",
        HostingAppType::Mhwp => {
            "Poll app provisioning status. The WordPress site starts behind a placeholder \
             page; log in to WordPress admin from Airo App Builder"
        }
    }
}

/// Sends the create with the request key saved for it, so that running the same
/// command again after a lost answer returns the first request's operation
/// instead of creating a second app (see [`create_keys`]).
///
/// The key is kept on every failure: after a clean refusal the service has let
/// it go and treats it as new, and after an unclear one it still holds it, so a
/// new key could create a second app. It is dropped once the create is
/// accepted with an operation, or when the service says it no longer fits this
/// request.
///
/// Dropping it after an accepted create is best effort: the create worked. After
/// a mismatch it is not: a key left in the file would be sent, and refused,
/// again, so the error then says how to clear it.
async fn submit(
    client: &HostingClient,
    keys: Option<&Path>,
    create: create_keys::Create<'_>,
) -> cli_engine::Result<Value> {
    let base_url = client.base_url();
    let app_type = create.app_type;
    let claim = create_keys::claim(keys, base_url, create, Utc::now());
    let result = client
        .create_app(
            app_type.as_str(),
            create.hosting_product.map(AppHostingProduct::as_str),
            json!({ "name": create.name }),
            &claim.key,
        )
        .await
        .and_then(require_operation_id);
    let mismatch = result.as_ref().err().is_some_and(is_key_mismatch);
    let released = if result.is_ok() || mismatch {
        create_keys::release(keys, base_url, create, Utc::now())
    } else {
        true
    };
    result.map_err(|e| {
        let key = KeyState {
            // Checked after the request: a run that started just before the key
            // expired can fail after it, and a run then would send a new key.
            rerun_until: claim.rerun_deadline(Utc::now()),
            unremoved: keys.filter(|_| !released),
        };
        create_err(e, app_type, &key)
    })
}

fn is_key_mismatch(e: &ClientError) -> bool {
    error_issues(e)
        .iter()
        .any(|issue| issue == "IDEMPOTENCY_KEY_MISMATCH")
}

/// What running the same command again does with this create's request key.
#[derive(Debug, Default)]
struct KeyState<'a> {
    /// The time before which the same command resends the key, so that the
    /// service returns the first request's result. `None` when it would send a
    /// new key: the key is not saved, or it expires too soon.
    rerun_until: Option<DateTime<Utc>>,
    /// The key file, when a key that has to go could not be removed from it.
    unremoved: Option<&'a Path>,
}

/// An accepted create always names its operation. An answer without one (an
/// empty reply from a proxy, say) does not show whether the create started, so
/// it is handled like a lost answer: the key is kept for the next run.
fn require_operation_id(data: Value) -> Result<Value, ClientError> {
    let has_id = data
        .get("operationId")
        .and_then(Value::as_str)
        .is_some_and(|id| !id.is_empty());
    if has_id {
        Ok(data)
    } else {
        Err(ClientError::Request(
            "the response did not include an operation ID".to_owned(),
        ))
    }
}

fn create_err(e: ClientError, app_type: HostingAppType, key: &KeyState<'_>) -> CliCoreError {
    match create_fix(&e, app_type, key) {
        Some(fix) => match e {
            http @ ClientError::Http { .. } => client_err_with_fix(http, fix),
            other => GddyError::from(other).with_fix(fix).into_cli_error(),
        },
        None => client_err(e),
    }
}

/// The next step for a failed create. A run of the same command is promised to
/// be safe only for a WordPress site whose request key is saved and not about
/// to expire, and only until a stated time: the Node.js service does not
/// recognize a repeated request yet.
fn create_fix(e: &ClientError, app_type: HostingAppType, key: &KeyState<'_>) -> Option<String> {
    let mhwp = app_type == HostingAppType::Mhwp;
    let list = format!("gddy hosting app list --app-type {}", app_type.as_str());
    if let Some(file) = key.unremoved
        && is_key_mismatch(e)
    {
        return Some(format!(
            "The saved request for this create could not be removed from {}. \
             Make sure you can write to that file and its folder. Then run the command again: \
             it clears the saved request and asks you to run it once more.",
            file.display()
        ));
    }
    let rerun = key
        .rerun_until
        .filter(|_| mhwp)
        .map(|until| until.format("%Y-%m-%d %H:%M UTC").to_string());
    issue_fix(e, mhwp, &list).or_else(|| match e {
        // A refused create started nothing, but it may be the re-run of one
        // whose answer was lost, so the wait must not outlast the saved key.
        ClientError::Http { status: 429, .. } => Some(rerun_after_wait(
            "Too many creates in a short time. Wait a minute",
            rerun.as_deref(),
            &list,
        )),
        // 501 keeps the shared "not supported" fix.
        ClientError::Http { status, .. } if *status >= 500 && *status != 501 => Some(
            rerun_after_wait("Wait a few seconds", rerun.as_deref(), &list),
        ),
        ClientError::Network(_) | ClientError::Request(_) | ClientError::Response(_) => {
            Some(if let Some(until) = &rerun {
                format!(
                    "The request may have reached GoDaddy. Run the same command again with the \
                     same --name before {until}. You get the first request's result, not a \
                     second site. After that time, first run: {list}."
                )
            } else {
                format!(
                    "The request may have reached GoDaddy. Before you run the command again, \
                     run: {list}, so you do not create a second app."
                )
            })
        }
        _ => None,
    })
}

/// The fix for an error worth waiting out: after `wait`, run the same command
/// again before `rerun` when that is safe, or check the list first.
fn rerun_after_wait(wait: &str, rerun: Option<&str>, list: &str) -> String {
    match rerun {
        Some(until) => format!(
            "{wait}, then run the same command again with the same --name before {until}. \
             You get the first request's result, not a second site. \
             After that time, first run: {list}."
        ),
        None => {
            format!("{wait}. Then run: {list}. Run the command again only if the app is not there.")
        }
    }
}

fn issue_fix(e: &ClientError, mhwp: bool, list: &str) -> Option<String> {
    error_issues(e).iter().find_map(|issue| match issue.as_str() {
        "SUBSCRIPTION_REQUIRED" if mhwp => Some(
            "MHWP needs an Airo App Builder subscription. Set one up, then run this command again."
                .to_owned(),
        ),
        "SUBSCRIPTION_INACTIVE" if mhwp => Some(
            "Your Airo App Builder subscription is not active. Reactivate it, then run this command again."
                .to_owned(),
        ),
        "WORDPRESS_NOT_ENABLED" if mhwp => Some(
            "WordPress apps are not enabled for this account yet. Contact GoDaddy support.".to_owned(),
        ),
        "MULTIPLE_SYSTEMS_AMBIGUOUS" if mhwp => Some(
            "Your Airo App Builder account has more than one hosting system. \
             Create the site in Airo App Builder instead."
                .to_owned(),
        ),
        // Sent for a create that asked for an Airo App Builder app, and for
        // one that left the choice open and has no Web Hosting plan.
        "AIRO_NOT_AVAILABLE" if !mhwp => Some(
            "A Node.js app on Airo App Builder is not available for this account. Check that \
             your Airo App Builder plan is active, then run this command again. To host the \
             app on a Web Hosting plan instead, run the command again with \
             --hosting-product WEB_HOSTING."
                .to_owned(),
        ),
        "APP_LIMIT_EXCEEDED" => Some(format!(
            "You reached the app limit. Delete an app you no longer need, then run this command again. \
             Run: {list}"
        )),
        // The first request with this name may still be starting, or its start
        // was unclear: the service holds the name's request for up to a day, but
        // a create that started shows in the list within 30 minutes.
        "IDEMPOTENCY_IN_PROGRESS" => Some(format!(
            "A create with this name is still being set up. Wait a few minutes, then run: {list}. \
             If the app is there, use it. If it is still not there after 30 minutes, \
             run the command again with a different --name."
        )),
        "IDEMPOTENCY_KEY_MISMATCH" => Some("Run the command again.".to_owned()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use cli_engine::Cli;

    use super::*;
    use crate::hosting::test_support::{cli, run_hosting};

    fn fix_of(err: &CliCoreError) -> Option<String> {
        cli_engine::build_error_envelope(err, "hosting").fix
    }

    fn until() -> DateTime<Utc> {
        "2026-11-14T12:00:59Z".parse().expect("valid time")
    }

    fn saved() -> KeyState<'static> {
        KeyState {
            rerun_until: Some(until()),
            unremoved: None,
        }
    }

    fn fix_for(status: u16, issue: &str, app_type: HostingAppType) -> Option<String> {
        let err = create_err(
            ClientError::Http {
                status,
                body: format!(r#"{{"message":"m","details":[{{"issue":"{issue}"}}]}}"#),
            },
            app_type,
            &saved(),
        );
        fix_of(&err)
    }

    #[test]
    fn create_err_gives_a_fix_for_each_create_issue() {
        for (status, issue, expected) in [
            (
                422,
                "SUBSCRIPTION_REQUIRED",
                "needs an Airo App Builder subscription",
            ),
            (403, "SUBSCRIPTION_INACTIVE", "subscription is not active"),
            (403, "WORDPRESS_NOT_ENABLED", "not enabled for this account"),
            (
                422,
                "MULTIPLE_SYSTEMS_AMBIGUOUS",
                "more than one hosting system",
            ),
            (
                422,
                "APP_LIMIT_EXCEEDED",
                "hosting app list --app-type MHWP",
            ),
            (409, "IDEMPOTENCY_IN_PROGRESS", "with a different --name"),
            (422, "IDEMPOTENCY_KEY_MISMATCH", "Run the command again."),
        ] {
            let fix = fix_for(status, issue, HostingAppType::Mhwp);
            assert!(
                fix.as_deref().is_some_and(|f| f.contains(expected)),
                "{issue}: {fix:?}"
            );
        }
    }

    #[test]
    fn create_err_finds_a_known_issue_after_other_details() {
        let err = create_err(
            ClientError::Http {
                status: 422,
                body: r#"{"message":"m","details":[{"issue":"VALIDATION_FAILED"},{"issue":"APP_LIMIT_EXCEEDED"}]}"#
                    .to_owned(),
            },
            HostingAppType::Mhwp,
            &saved(),
        );
        let fix = fix_of(&err);
        assert!(
            fix.as_deref().is_some_and(|f| f.contains("app limit")),
            "{fix:?}"
        );
    }

    const SAFE: &str = "You get the first request's result, not a second site.";

    #[test]
    fn a_rerun_is_promised_safe_only_for_a_saved_wordpress_create() {
        let network = || ClientError::Network("timed out".to_owned());
        let server = || ClientError::Http {
            status: 503,
            body: r#"{"message":"m","details":[{"issue":"UPSTREAM_UNAVAILABLE"}]}"#.to_owned(),
        };
        for make in [network, server] {
            let safe = fix_of(&create_err(make(), HostingAppType::Mhwp, &saved()));
            // The minute is rounded down, so the time given is never late.
            assert!(
                safe.as_deref().is_some_and(|f| f.contains(SAFE)
                    && f.contains("same --name before 2026-11-14 12:00 UTC.")
                    && f.contains("After that time, first run: gddy hosting app list")),
                "{safe:?}"
            );

            for (app_type, key) in [
                (HostingAppType::Mhwp, KeyState::default()),
                (HostingAppType::Nodejs, saved()),
                (HostingAppType::Nodejs, KeyState::default()),
            ] {
                let fix = fix_of(&create_err(make(), app_type, &key));
                let list = format!("gddy hosting app list --app-type {}", app_type.as_str());
                assert!(
                    fix.as_deref().is_some_and(|f| f.contains(&list)
                        && !f.contains(SAFE)
                        && !f.contains("same --name")),
                    "{app_type:?} {key:?}: {fix:?}"
                );
            }
        }
    }

    #[test]
    fn a_mismatched_key_that_could_not_be_removed_says_how_to_clear_it() {
        let file = Path::new("/home/me/.config/gddy/hosting-create-keys.json");
        let mismatch = || ClientError::Http {
            status: 422,
            body: r#"{"message":"m","details":[{"issue":"IDEMPOTENCY_KEY_MISMATCH"}]}"#.to_owned(),
        };
        for app_type in [HostingAppType::Mhwp, HostingAppType::Nodejs] {
            let key = KeyState {
                rerun_until: Some(until()),
                unremoved: Some(file),
            };
            let fix = fix_of(&create_err(mismatch(), app_type, &key));
            assert!(
                fix.as_deref()
                    .is_some_and(|f| f.contains(&file.display().to_string())
                        && f.contains("Make sure you can write to that file")),
                "{app_type:?}: {fix:?}"
            );
        }

        // Only a mismatch needs the key gone; other errors keep their own fix.
        let key = KeyState {
            rerun_until: Some(until()),
            unremoved: Some(file),
        };
        let network = ClientError::Network("timed out".to_owned());
        let fix = fix_of(&create_err(network, HostingAppType::Mhwp, &key));
        assert!(fix.as_deref().is_some_and(|f| f.contains(SAFE)), "{fix:?}");
    }

    #[test]
    fn create_err_tells_to_wait_on_a_rate_limit() {
        let limited = || ClientError::Http {
            status: 429,
            body: String::new(),
        };
        let fix = fix_of(&create_err(limited(), HostingAppType::Mhwp, &saved()));
        assert!(
            fix.as_deref().is_some_and(|f| f.contains("Wait a minute")
                && f.contains(SAFE)
                && f.contains("same --name before 2026-11-14 12:00 UTC.")),
            "{fix:?}"
        );

        // No saved key, or one about to expire, or Node.js: list first, as
        // the wait could outlast a replay of an earlier create.
        for (app_type, key) in [
            (HostingAppType::Mhwp, KeyState::default()),
            (HostingAppType::Nodejs, saved()),
        ] {
            let fix = fix_of(&create_err(limited(), app_type, &key));
            let list = format!("gddy hosting app list --app-type {}", app_type.as_str());
            assert!(
                fix.as_deref().is_some_and(|f| f.contains("Wait a minute")
                    && f.contains(&list)
                    && !f.contains("same --name")),
                "{app_type:?}: {fix:?}"
            );
        }
    }

    #[test]
    fn create_err_keeps_the_not_supported_fix_for_501() {
        let fix = fix_for(501, "NOT_APPLICABLE", HostingAppType::Mhwp);
        assert!(
            fix.as_deref().is_some_and(|f| f.contains("not supported")),
            "{fix:?}"
        );
    }

    #[test]
    fn create_err_keeps_default_fix_for_other_errors() {
        let fix = fix_for(403, "SOMETHING_ELSE", HostingAppType::Mhwp);
        assert!(
            fix.as_deref().is_some_and(|f| f.contains("auth scope")),
            "{fix:?}"
        );
    }

    #[test]
    fn create_err_keeps_wordpress_hints_off_nodejs() {
        let fix = fix_for(403, "WORDPRESS_NOT_ENABLED", HostingAppType::Nodejs);
        assert!(
            fix.as_deref().is_some_and(|f| f.contains("auth scope")),
            "{fix:?}"
        );
        let fix = fix_for(422, "APP_LIMIT_EXCEEDED", HostingAppType::Nodejs);
        assert!(
            fix.as_deref()
                .is_some_and(|f| f.contains("--app-type NODEJS")),
            "{fix:?}"
        );
    }

    #[test]
    fn create_err_explains_an_airo_app_that_is_not_available() {
        let fix = fix_for(422, "AIRO_NOT_AVAILABLE", HostingAppType::Nodejs);
        assert!(
            fix.as_deref().is_some_and(|f| {
                f.contains("Airo App Builder plan is active")
                    && f.contains("--hosting-product WEB_HOSTING")
            }),
            "{fix:?}"
        );
        // Another 422 on a Node.js create keeps the default fix.
        let fix = fix_for(422, "SOMETHING_ELSE", HostingAppType::Nodejs);
        assert!(
            fix.as_deref().is_some_and(|f| !f.contains("Airo")),
            "{fix:?}"
        );
    }

    #[test]
    fn poll_description_adds_wordpress_notes_for_mhwp() {
        assert_eq!(
            poll_description(HostingAppType::Nodejs),
            "Poll app provisioning status"
        );
        assert!(poll_description(HostingAppType::Mhwp).contains("placeholder page"));
    }

    async fn run(cli: &Cli, args: &[&str]) -> String {
        let mut argv = vec!["app"];
        argv.extend_from_slice(args);
        run_hosting(cli, &argv).await
    }

    const PARSED: &str = "no provider registered";

    #[tokio::test]
    async fn mhwp_is_accepted_and_documented() {
        let cli = cli();
        let create = run(&cli, &["create", "--app-type", "mhwp", "--name", "s"]).await;
        assert!(create.contains(PARSED), "{create}");
        let list = run(&cli, &["list", "--app-type", "MHWP"]).await;
        assert!(list.contains(PARSED), "{list}");
        let nodejs = run(&cli, &["create", "--app-type", "nodejs", "--name", "s"]).await;
        assert!(nodejs.contains(PARSED), "{nodejs}");

        let help = run(&cli, &["create", "--help"]).await;
        assert!(
            help.contains("Provision a new hosting application"),
            "{help}"
        );
        assert!(help.contains("Airo App Builder subscription"), "{help}");
        assert!(help.contains("hosting runtime get"), "{help}");
        assert!(help.contains("not available for WordPress sites"), "{help}");
        assert!(
            !help.contains("not available for WordPress sites yet"),
            "{help}"
        );
        assert!(help.contains("possible values: NODEJS, MHWP"), "{help}");
    }

    #[test]
    fn a_hosting_product_is_refused_for_a_wordpress_site() {
        let args = |app_type, hosting_product| AppCreateArgs {
            app_type,
            name: "s".to_owned(),
            hosting_product,
        };
        let err = args(HostingAppType::Mhwp, Some(AppHostingProduct::Airo))
            .create()
            .expect_err("refused");
        assert_eq!(
            err.to_string(),
            "--hosting-product applies to --app-type NODEJS only, not MHWP"
        );
        let fix = fix_of(&err.into_cli_error());
        assert_eq!(
            fix.as_deref(),
            Some("Run the command again without --hosting-product.")
        );

        for (app_type, product) in [
            (HostingAppType::Mhwp, None),
            (HostingAppType::Nodejs, None),
            (HostingAppType::Nodejs, Some(AppHostingProduct::Airo)),
            (HostingAppType::Nodejs, Some(AppHostingProduct::WebHosting)),
        ] {
            let args = args(app_type, product);
            let create = args.create().expect("accepted");
            assert_eq!(create.app_type, app_type);
            assert_eq!(create.hosting_product, product);
            assert_eq!(create.name, "s");
        }
    }

    #[tokio::test]
    async fn hosting_product_is_a_documented_nodejs_only_choice() {
        let cli = cli();
        for product in ["AIRO", "web_hosting"] {
            let out = run(
                &cli,
                &[
                    "create",
                    "--app-type",
                    "NODEJS",
                    "--name",
                    "s",
                    "--hosting-product",
                    product,
                ],
            )
            .await;
            assert!(out.contains(PARSED), "{product}: {out}");
        }

        // Not a product an app is created on: refused while parsing.
        let unknown = run(
            &cli,
            &[
                "create",
                "--app-type",
                "NODEJS",
                "--name",
                "s",
                "--hosting-product",
                "MANAGED_WORDPRESS",
            ],
        )
        .await;
        assert!(
            unknown.contains("possible values: WEB_HOSTING, AIRO"),
            "{unknown}"
        );
        assert!(!unknown.contains(PARSED), "{unknown}");

        let help = run(&cli, &["create", "--help"]).await;
        assert!(help.contains("--hosting-product <PRODUCT>"), "{help}");
        assert!(
            help.contains("possible values: WEB_HOSTING, AIRO"),
            "{help}"
        );
        assert!(help.contains("has no plan to attach"), "{help}");
    }
}

#[cfg(test)]
#[path = "create_flow_tests.rs"]
mod flow_tests;
