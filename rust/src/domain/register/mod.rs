//! `gddy domain register` — interactive guided domain registration wizard.
//!
//! Walks the user through discovery → configure → confirm → buy in a single
//! session. In non-interactive mode, all options must be passed as flags;
//! the command validates them and executes directly without prompts.

use cli_engine::{
    CliCoreError, CommandContext, CommandResult, CommandSpec, Envelope, NextActionParam, Result,
    RuntimeCommandSpec, TableColumn, Tier, render_human_with_view,
};
use serde_json::json;

use crate::domain::common::{is_terminal_status, resolve_domain_name, validate_domain_name};
use crate::next_action::{next_action, required_value};
use crate::output_schema::output_schema;
use crate::scopes::{DOMAINS_CREATE, DOMAINS_READ};

// Wizard steps write interactive UI to stderr via eprintln and dialoguer/console.
// This is intentional user-facing output, not diagnostic logging.
#[allow(clippy::print_stderr)]
pub(crate) mod bridge;
#[allow(clippy::print_stderr)]
pub(crate) mod steps;
#[allow(clippy::print_stderr)]
pub(crate) mod wizard;

use wizard::{StepContext, WizardState};

output_schema!(DomainRegisterResult {
    "domain": "string";
    "status": "string";
    "operationId": "string", optional;
    "price": "string", optional;
    "currency": "string", optional;
});

fn view_columns() -> Vec<TableColumn> {
    vec![
        TableColumn::new("domain", "Domain"),
        TableColumn::new("status", "Status"),
        TableColumn::new("operationId", "Operation ID").no_truncate(true),
        TableColumn::new("price", "Price"),
        TableColumn::new("currency", "Currency"),
    ]
}

#[derive(Debug, Clone, clap::Args)]
struct RegisterArgs {
    /// Domain name to register (omit for interactive discovery).
    #[arg(value_name = "DOMAIN")]
    domain: Option<String>,

    /// Registration period in years (default: 1).
    #[arg(long, default_value = "1", value_name = "YEARS")]
    period: u64,

    /// Enable WHOIS privacy protection.
    #[arg(long, default_value = "true", action = clap::ArgAction::Set)]
    privacy: bool,

    /// Enable automatic renewal.
    #[arg(long = "auto-renew", default_value = "true", action = clap::ArgAction::Set)]
    auto_renew: bool,

    /// Custom nameserver (repeatable; omit for GoDaddy defaults).
    #[arg(long = "nameserver", value_name = "HOST")]
    nameservers: Vec<String>,

    /// Consent to legal agreements (required in non-interactive mode).
    #[arg(long)]
    agree: bool,

    /// Confirm the purchase (required in non-interactive mode).
    #[arg(long)]
    confirm: bool,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<RegisterArgs, _, _, _>(
        CommandSpec::from_args::<RegisterArgs>(
            "register",
            "Register a new domain (interactive wizard or direct)",
        )
        .with_long(
            "Register a new domain interactively or with flags.\n\
             \n\
             In interactive mode (default when running in a terminal), the wizard\n\
             walks you through: domain discovery → registration options → quote\n\
             review → purchase execution.\n\
             \n\
             In non-interactive mode (piped input, CI, or --non-interactive), pass\n\
             the domain name and all options as flags:\n\
             \n  \
             gddy domain register example.com --period 1 --agree --confirm\n\
             \n\
             Registration charges your GoDaddy account and cannot be undone.\n\
             A usable payment method must be on file.",
        )
        .with_system("domain")
        .with_tier(Tier::Destructive)
        .with_default_fields("domain,status,operationId,price,currency")
        .with_output_schema::<DomainRegisterResult>()
        .with_view(view_columns())
        .with_scopes(&[DOMAINS_READ, DOMAINS_CREATE]),
        |ctx, args: RegisterArgs| async move {
            let is_interactive = ctx.is_interactive();

            if is_interactive {
                run_interactive(ctx, args).await
            } else {
                run_non_interactive(ctx, args).await
            }
        },
    )
}

async fn run_interactive(ctx: CommandContext, args: RegisterArgs) -> Result<CommandResult> {
    let cred = ctx.credential().await?;
    let env = ctx.middleware.env.clone();
    let debug = !ctx.middleware.debug.is_empty();

    // Pre-populate state from any flags the user already provided.
    let domain = match args.domain {
        Some(d) => Some(resolve_domain_name(
            &ctx,
            &d,
            "Enter domain name to register (e.g. example.com)",
        )?),
        None => None,
    };

    let state = WizardState::new()
        .with_domain(domain)
        .with_period(args.period)
        .with_privacy(args.privacy)
        .with_auto_renew(args.auto_renew)
        .with_nameservers(args.nameservers);

    let step_ctx = StepContext {
        credential: cred,
        env,
        debug,
        wizard_start_at: 0,
    };

    let final_state = wizard::run_wizard(state, step_ctx, 0).await?;

    if final_state.cancelled {
        return Ok(CommandResult::new(json!({"status": "cancelled"})));
    }
    build_result(&final_state)
}

async fn run_non_interactive(ctx: CommandContext, args: RegisterArgs) -> Result<CommandResult> {
    let domain = args.domain.ok_or_else(|| {
        CliCoreError::message(
            "domain name is required in non-interactive mode; pass it as a positional argument\n\
             \n  Example: gddy domain register example.com --period 1 --agree --confirm",
        )
    })?;
    let domain = validate_domain_name(&domain)?;

    if !args.agree {
        return Err(CliCoreError::message(
            "--agree is required in non-interactive mode to consent to legal agreements",
        ));
    }
    if !args.confirm {
        return Err(CliCoreError::message(
            "--confirm is required in non-interactive mode to authorize the purchase charge",
        ));
    }

    let cred = ctx.credential().await?;
    let env = ctx.middleware.env.clone();
    let debug = !ctx.middleware.debug.is_empty();

    let mut state = WizardState::new()
        .with_domain(Some(domain))
        .with_period(args.period)
        .with_privacy(args.privacy)
        .with_auto_renew(args.auto_renew)
        .with_nameservers(args.nameservers);
    state.available = true;

    let step_ctx = StepContext {
        credential: cred,
        env,
        debug,
        wizard_start_at: 0,
    };

    // Non-interactive: skip all interactive prompts. --agree and --confirm
    // were validated above, so we go straight to quoting and executing.
    steps::review::run_non_interactive(&mut state, &step_ctx).await?;
    steps::execute::run(&mut state, &step_ctx).await?;

    build_result(&state)
}

/// Result of an interactive bridge from another domain command into the wizard.
pub(crate) enum BridgeHandoff {
    /// Replace the host command's normal stdout output (wizard completed or cancelled).
    Replace(CommandResult),
    /// Run the host command's normal output (user declined the bridge prompt).
    ShowHostOutput,
}

/// Host-command output after the user cancels the wizard. The wizard already
/// wrote a cancellation message to stderr; stdout should stay quiet in human mode.
pub(crate) fn cancelled_host_result(ctx: &CommandContext) -> CommandResult {
    if ctx.middleware.output_format == "human" {
        CommandResult::new(json!(""))
    } else {
        CommandResult::new(json!({"status": "cancelled"}))
    }
}

/// Wizard exit disposition, distinguishing user-initiated back-navigation from
/// explicit cancellation.
pub(crate) enum WizardExit {
    /// Wizard completed — here's the result.
    Completed(CommandResult),
    /// User navigated back past the entry step (caller should re-show its UI).
    BackedOut,
    /// User explicitly cancelled (Cancel option or Ctrl+C). The wizard already
    /// printed a user-facing message; callers should exit without rendering
    /// additional output.
    Cancelled,
}

/// Launch the wizard from an external command (e.g. `domain available --interactive`).
/// `start_at` determines which step to begin from (0=discovery, 1=options, etc.).
pub(crate) async fn launch_wizard(
    ctx: &CommandContext,
    state: WizardState,
    start_at: usize,
) -> Result<WizardExit> {
    let cred = ctx.credential().await?;
    let env = ctx.middleware.env.clone();
    let debug = !ctx.middleware.debug.is_empty();

    let step_ctx = StepContext {
        credential: cred,
        env,
        debug,
        wizard_start_at: start_at,
    };

    let final_state = wizard::run_wizard(state, step_ctx, start_at).await?;
    if final_state.backed_out {
        return Ok(WizardExit::BackedOut);
    }
    if final_state.cancelled {
        return Ok(WizardExit::Cancelled);
    }
    build_result(&final_state).map(WizardExit::Completed)
}

fn build_result(state: &WizardState) -> Result<CommandResult> {
    let domain = state
        .domain
        .as_ref()
        .ok_or_else(|| CliCoreError::message("no domain in final state"))?;
    let status = state.status.as_deref().unwrap_or("UNKNOWN");

    let mut result = json!({
        "domain": domain,
        "status": status,
    });
    if let Some(op) = &state.operation_id {
        result["operationId"] = json!(op);
    }
    if let Some(p) = &state.price {
        result["price"] = json!(p);
    }
    if let Some(c) = &state.currency {
        result["currency"] = json!(c);
    }

    let mut actions = vec![
        next_action("domain get <domain>", next_action_description(status))
            .with_param("domain", required_value(domain.clone())),
        next_action(
            "dns set <domain> --type A --name @ --data <ip>",
            "Point the apex at an IPv4 address",
        )
        .with_param("domain", required_value(domain.clone()))
        .with_param("ip", NextActionParam::required()),
    ];
    // Mirror `domain purchase`: when polling gave up before a terminal state,
    // surface the operation-status follow-up with the concrete id prefilled.
    if !is_terminal_status(status)
        && let Some(op) = &state.operation_id
    {
        actions.push(
            next_action(
                "domain operation <operation-id>",
                "Check whether registration has finished since polling gave up",
            )
            .with_param("operation-id", required_value(op.clone())),
        );
    }

    Ok(CommandResult::new(result).with_next_actions(actions))
}

fn next_action_description(status: &str) -> &'static str {
    if status == "COMPLETED" {
        "See the registered domain's details"
    } else {
        "Check whether the domain has finished registering"
    }
}

/// Adapt a wizard `CommandResult` for return through a different leaf command
/// (`domain suggest` / `available` / `quote`).
///
/// Those hosts register their own human views (e.g. suggest's `1yr Price`
/// columns). Rendering a register-shaped payload through them produces empty
/// mismatched fields. For human output we render with the register view
/// ourselves, then return an empty string so the host view path is bypassed
/// (scalar data skips column rendering). JSON/TOON keep the structured result.
pub(crate) fn present_for_host_command(
    ctx: &CommandContext,
    result: CommandResult,
) -> CommandResult {
    if ctx.middleware.output_format != "human" {
        return result;
    }

    let envelope =
        Envelope::success(result.data, "domain").with_next_actions(result.metadata.next_actions);
    let rendered = render_human_with_view(&envelope, Some(&view_columns()), "");
    // Write the correctly shaped register summary now; middleware will then
    // render the empty-string placeholder below (a blank line), not the host
    // command's mismatched table. Use Write rather than print! so the
    // print_stdout lint (denied as warnings) does not fire on intentional
    // human-output writes.
    let _ = std::io::Write::write_all(&mut std::io::stdout(), rendered.as_bytes());
    CommandResult::new(json!(""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wizard::WizardState;

    #[test]
    fn build_result_requires_domain_in_state() {
        let state = WizardState::new();
        let err = build_result(&state).expect_err("should fail without domain");
        assert!(
            err.to_string().contains("no domain"),
            "expected domain error, got: {err}"
        );
    }

    #[test]
    fn build_result_produces_valid_json_with_minimal_state() {
        let mut state = WizardState::new();
        state.domain = Some("example.com".to_string());
        state.status = Some("COMPLETED".to_string());

        let result = build_result(&state).expect("should succeed");
        assert_eq!(result.data["domain"], "example.com");
        assert_eq!(result.data["status"], "COMPLETED");
        assert!(result.data.get("operationId").is_none());
    }

    #[test]
    fn build_result_includes_optional_fields_when_present() {
        let mut state = WizardState::new();
        state.domain = Some("test.io".to_string());
        state.status = Some("COMPLETED".to_string());
        state.operation_id = Some("op-123".to_string());
        state.price = Some("12.99".to_string());
        state.currency = Some("USD".to_string());

        let result = build_result(&state).expect("should succeed");
        assert_eq!(result.data["operationId"], "op-123");
        assert_eq!(result.data["price"], "12.99");
        assert_eq!(result.data["currency"], "USD");
    }

    #[test]
    fn build_result_prefills_domain_in_next_actions() {
        let mut state = WizardState::new();
        state.domain = Some("example.com".to_string());
        state.status = Some("COMPLETED".to_string());

        let result = build_result(&state).expect("should succeed");
        assert!(
            !result.metadata.next_actions.is_empty(),
            "expected next actions"
        );
        let get = &result.metadata.next_actions[0];
        let domain = get
            .params
            .get("domain")
            .and_then(|p| p.value.as_deref())
            .expect("domain param");
        assert_eq!(domain, "example.com");
        // Substitution of `<domain>` → `example.com` happens at render time
        // via the envelope's next_actions footer; the stored template keeps
        // the placeholder, with the concrete value in params.
        assert!(get.command.contains("<domain>"), "{}", get.command);
        assert!(get.params.get("domain").is_some_and(|p| p.required));
    }

    #[test]
    fn build_result_adds_operation_status_when_still_pending() {
        let mut state = WizardState::new();
        state.domain = Some("example.com".to_string());
        state.status = Some("EXECUTING".to_string());
        state.operation_id = Some("op-abc".to_string());

        let result = build_result(&state).expect("should succeed");
        let status_action = result
            .metadata
            .next_actions
            .iter()
            .find(|a| a.command.contains("domain operation"))
            .expect("pending registration should suggest operation status");
        assert_eq!(
            status_action
                .params
                .get("operation-id")
                .and_then(|p| p.value.as_deref()),
            Some("op-abc")
        );
    }

    #[test]
    fn register_view_renders_register_shaped_payload() {
        let payload = json!({
            "domain": "example.com",
            "status": "COMPLETED",
            "operationId": "op-1",
            "price": "12.99",
            "currency": "USD",
        });
        let envelope = Envelope::success(payload, "domain");
        let rendered = render_human_with_view(&envelope, Some(&view_columns()), "");
        assert!(rendered.contains("Domain:"), "{rendered}");
        assert!(rendered.contains("example.com"), "{rendered}");
        assert!(rendered.contains("Status:"), "{rendered}");
        assert!(rendered.contains("COMPLETED"), "{rendered}");
        assert!(rendered.contains("12.99"), "{rendered}");
        assert!(
            !rendered.contains("1yr Price"),
            "must not use suggest's view labels: {rendered}"
        );
    }

    #[test]
    fn register_args_defaults_are_user_friendly() {
        // Verify clap defaults match WizardState defaults.
        let cmd = clap::Command::new("test");
        let cmd = <RegisterArgs as clap::Args>::augment_args(cmd);

        // period default is "1"
        let period_arg = cmd.get_arguments().find(|a| a.get_id() == "period");
        assert!(period_arg.is_some());

        // privacy default is "true"
        let privacy_arg = cmd.get_arguments().find(|a| a.get_id() == "privacy");
        assert!(privacy_arg.is_some());
    }

    #[test]
    fn non_interactive_requires_domain_arg() {
        let args = RegisterArgs {
            domain: None,
            period: 1,
            privacy: true,
            auto_renew: true,
            nameservers: vec![],
            agree: true,
            confirm: true,
        };
        // Simulate the check from run_non_interactive.
        let err = args.domain.ok_or_else(|| {
            CliCoreError::message("domain name is required in non-interactive mode")
        });
        assert!(err.is_err());
        assert!(
            err.expect_err("should be missing domain")
                .to_string()
                .contains("domain name is required")
        );
    }

    #[test]
    fn non_interactive_requires_agree_flag() {
        let args = RegisterArgs {
            domain: Some("example.com".to_string()),
            period: 1,
            privacy: true,
            auto_renew: true,
            nameservers: vec![],
            agree: false,
            confirm: true,
        };
        assert!(!args.agree, "--agree should be false");
    }

    #[test]
    fn non_interactive_requires_confirm_flag() {
        let args = RegisterArgs {
            domain: Some("example.com".to_string()),
            period: 1,
            privacy: true,
            auto_renew: true,
            nameservers: vec![],
            agree: true,
            confirm: false,
        };
        assert!(!args.confirm, "--confirm should be false");
    }

    #[test]
    fn wizard_state_from_args_maps_correctly() {
        let args = RegisterArgs {
            domain: Some("test.io".to_string()),
            period: 3,
            privacy: false,
            auto_renew: false,
            nameservers: vec!["ns1.test.io".to_string(), "ns2.test.io".to_string()],
            agree: true,
            confirm: true,
        };
        let state = WizardState::new()
            .with_period(args.period)
            .with_privacy(args.privacy)
            .with_auto_renew(args.auto_renew)
            .with_nameservers(args.nameservers.clone());

        assert_eq!(state.period, 3);
        assert!(!state.privacy);
        assert!(!state.auto_renew);
        assert_eq!(state.nameservers.len(), 2);
    }
}
