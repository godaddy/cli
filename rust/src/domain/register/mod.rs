//! `gddy domain register` — interactive guided domain registration wizard.
//!
//! Walks the user through discovery → configure → confirm → buy in a single
//! session. In non-interactive mode, all options must be passed as flags;
//! the command validates them and executes directly without prompts.

use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, NextActionParam, Result, RuntimeCommandSpec, Tier,
};
use serde_json::json;

use crate::domain::common::validate_domain_name;
use crate::next_action::next_action;
use crate::output_schema::output_schema;
use crate::scopes::{DOMAINS_CREATE, DOMAINS_READ};

// Wizard steps write interactive UI to stderr via eprintln and dialoguer/console.
// This is intentional user-facing output, not diagnostic logging.
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

async fn run_interactive(
    ctx: cli_engine::CommandContext,
    args: RegisterArgs,
) -> Result<CommandResult> {
    let cred = ctx.credential().await?;
    let env = ctx.middleware.env.clone();
    let debug = !ctx.middleware.debug.is_empty();

    // Pre-populate state from any flags the user already provided.
    let domain = args.domain.map(|d| validate_domain_name(&d)).transpose()?;

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
    };

    let final_state = wizard::run_wizard(state, step_ctx, 0).await?;

    build_result(&final_state)
}

async fn run_non_interactive(
    ctx: cli_engine::CommandContext,
    args: RegisterArgs,
) -> Result<CommandResult> {
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

    let state = WizardState::new()
        .with_domain(Some(domain))
        .with_period(args.period)
        .with_privacy(args.privacy)
        .with_auto_renew(args.auto_renew)
        .with_nameservers(args.nameservers);

    let step_ctx = StepContext {
        credential: cred,
        env,
        debug,
    };

    // In non-interactive mode, we skip the wizard UI and execute the steps
    // directly (availability check → quote → register), relying on flags for
    // all configuration.
    let final_state = wizard::run_wizard(state, step_ctx, 0).await?;

    build_result(&final_state)
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

    let actions = vec![
        next_action("domain get <domain>", "See the registered domain's details")
            .with_param("domain", NextActionParam::required()),
    ];

    Ok(CommandResult::new(result).with_next_actions(actions))
}
