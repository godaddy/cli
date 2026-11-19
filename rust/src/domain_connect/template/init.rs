//! `gddy domain-connect template init` — write a template file.

use std::io::IsTerminal;
use std::path::Path;

use cli_engine::{
    CommandContext, CommandResult, CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec,
    TableColumn, Tier,
};
use serde_json::json;

use super::interactive::prompt_template;
use super::parsers::parse_records;
use super::types::{Essential, RecordKind, Template};
use crate::error::GddyError;
use crate::next_action::next_action;
use crate::output_schema::output_schema;

const SYSTEM: &str = "domain-connect";

output_schema!(InitOutput {
    "file": "string";
    "action": "string";
    "providerId": "string";
    "serviceId": "string";
    "recordCount": "number";
    "template": "object";
});

#[derive(Debug, Clone, clap::Args)]
struct InitArgs {
    /// Build the template from flags instead of asking questions
    #[arg(long = "no-interactive")]
    no_interactive: bool,

    /// Your domain name, which identifies you as the provider, e.g. acme.com
    #[arg(long = "provider-id", value_name = "DOMAIN")]
    provider_id: Option<String>,

    /// Your name as customers see it, e.g. "Acme Corp"
    #[arg(long = "provider-name", value_name = "NAME")]
    provider_name: Option<String>,

    /// Short identifier for the service, e.g. managed-wordpress
    #[arg(long = "service-id", value_name = "ID")]
    service_id: Option<String>,

    /// The service's name as customers see it, e.g. "Acme Managed WordPress"
    #[arg(long = "service-name", value_name = "NAME")]
    service_name: Option<String>,

    /// What the template sets up, e.g. "Points your domain at Acme hosting"
    #[arg(long = "description", value_name = "TEXT")]
    description: Option<String>,

    /// Replace the template file if it already exists
    #[arg(long = "force")]
    force: bool,

    /// Link to your logo; an http or https URL ending in .jpg, .png or .svg
    #[arg(long = "logo-url", value_name = "URL")]
    logo_url: Option<String>,

    /// Describes the template's variables, e.g. "ip: your server's IPv4 address"
    #[arg(long = "variable-description", value_name = "TEXT")]
    variable_description: Option<String>,

    /// Domain that publishes your signing key, e.g. keys.acme.com
    #[arg(long = "sync-pub-key-domain", value_name = "DOMAIN")]
    sync_pub_key_domain: Option<String>,

    /// Domains customers may be sent back to after setup, comma-separated, e.g. acme.com,app.acme.com
    #[arg(long = "sync-redirect-domain", value_name = "DOMAINS")]
    sync_redirect_domain: Option<String>,

    /// Turn off the in-browser (synchronous) setup flow for this template (true or false)
    #[arg(long = "sync-block", value_name = "BOOL")]
    sync_block: Option<bool>,

    /// Older form of --shared-provider-name; set both for DNS providers that only read this one (true or false)
    #[arg(long = "shared", value_name = "BOOL")]
    shared: Option<bool>,

    /// Let extra text be added to the provider name when the template is applied (true or false)
    #[arg(long = "shared-provider-name", value_name = "BOOL")]
    shared_provider_name: Option<bool>,

    /// Let extra text be added to the service name when the template is applied (true or false)
    #[arg(long = "shared-service-name", value_name = "BOOL")]
    shared_service_name: Option<bool>,

    /// Allow the template to be applied more than once to a domain (true or false)
    #[arg(long = "multi-instance", value_name = "BOOL")]
    multi_instance: Option<bool>,

    /// Warn customers about phishing when the template cannot be signed (true or false)
    #[arg(long = "warn-phishing", value_name = "BOOL")]
    warn_phishing: Option<bool>,

    /// Require a host name when the template is applied (true or false)
    #[arg(long = "host-required", value_name = "BOOL")]
    host_required: Option<bool>,

    /// Add an A record (repeatable), e.g. "host=@,value=192.0.2.1" or "host=www,value=ip,ttl=600"
    #[arg(long = "add-a", value_name = "RECORD")]
    add_a: Vec<String>,

    /// Add an AAAA record (repeatable), e.g. "host=@,value=2001:db8::1" or "value=ip6"
    #[arg(long = "add-aaaa", value_name = "RECORD")]
    add_aaaa: Vec<String>,

    /// Add a CNAME record (repeatable), e.g. "host=www,value=sites.acme.com"
    #[arg(long = "add-cname", value_name = "RECORD")]
    add_cname: Vec<String>,

    /// Add an MX record (repeatable), e.g. "value=mail.acme.com,priority=10"
    #[arg(long = "add-mx", value_name = "RECORD")]
    add_mx: Vec<String>,

    /// Add a TXT record (repeatable), e.g. "host=@,data=v=spf1 include:acme.com ~all"
    #[arg(long = "add-txt", value_name = "RECORD")]
    add_txt: Vec<String>,

    /// Add an SRV record (repeatable), e.g. "service=_sip,protocol=_tcp,target=sip.acme.com,port=5060"
    #[arg(long = "add-srv", value_name = "RECORD")]
    add_srv: Vec<String>,

    /// Add an NS record (repeatable), e.g. "host=shop,value=ns1.acme.com"
    #[arg(long = "add-ns", value_name = "RECORD")]
    add_ns: Vec<String>,

    /// Add SPF rules merged into the domain's SPF record (repeatable), e.g. "rules=include:_spf.acme.com"
    #[arg(long = "add-spfm", value_name = "RECORD")]
    add_spfm: Vec<String>,

    /// Add a permanent redirect (repeatable), e.g. "host=@,target=https://www.acme.com"
    #[arg(long = "add-redir301", value_name = "RECORD")]
    add_redir301: Vec<String>,

    /// Add a temporary redirect (repeatable), e.g. "host=@,target=https://www.acme.com"
    #[arg(long = "add-redir302", value_name = "RECORD")]
    add_redir302: Vec<String>,

    /// Add an alias at the root of the domain (repeatable), e.g. "value=sites.acme.com"
    #[arg(long = "add-apexcname", value_name = "RECORD")]
    add_apexcname: Vec<String>,

    /// Add a CAA record (repeatable), e.g. "data=0 issue \"letsencrypt.org\""
    #[arg(long = "add-caa", value_name = "RECORD")]
    add_caa: Vec<String>,

    /// Group ID for every record that does not set its own with group=, e.g. web
    #[arg(long = "group-id", value_name = "ID")]
    group_id: Option<String>,

    /// Whether records must stay in place (Always) or may be removed after setup (OnApply)
    #[arg(long = "essential", value_enum, ignore_case = true)]
    essential: Option<Essential>,
}

pub(super) fn check_provider_id(provider_id: &str) -> Result<(), GddyError> {
    match crate::domain::common::validate_domain_name(provider_id) {
        Ok(normalized) if normalized == provider_id => Ok(()),
        _ => Err(GddyError::validation(format!(
            "Provider ID '{provider_id}' must be a lowercase domain name, such as acme.com."
        ))
        .with_fix("Use the domain name you own, in lowercase, for example acme.com.")
        .with_system(SYSTEM)),
    }
}

pub(super) fn check_service_id(service_id: &str) -> Result<(), GddyError> {
    let allowed = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_';
    if !service_id.is_empty() && service_id.chars().all(allowed) {
        return Ok(());
    }
    Err(GddyError::validation(format!(
        "Service ID '{service_id}' may contain only lowercase letters, digits, hyphens and underscores."
    ))
    .with_fix("Use a short identifier such as managed-wordpress.")
    .with_system(SYSTEM))
}

fn check_logo_url(logo_url: &str) -> Result<(), GddyError> {
    let path_ok = url::Url::parse(logo_url).ok().is_some_and(|parsed| {
        let path = parsed.path().to_ascii_lowercase();
        matches!(parsed.scheme(), "http" | "https")
            && [".jpg", ".png", ".svg"]
                .iter()
                .any(|extension| path.ends_with(extension))
    });
    if path_ok {
        return Ok(());
    }
    Err(GddyError::validation(format!(
        "Logo URL '{logo_url}' must be an http or https link to a .jpg, .png or .svg file."
    ))
    .with_fix("Pass a link such as https://acme.com/logo.svg to --logo-url.")
    .with_system(SYSTEM))
}

fn missing_identity(flag: &str) -> cli_engine::CliCoreError {
    GddyError::validation(format!("--{flag} is required with --no-interactive."))
        .with_fix(
            "Pass --provider-id, --provider-name, --service-id and --service-name, or run \
             without --no-interactive in a terminal to answer prompts.",
        )
        .with_system(SYSTEM)
        .into_cli_error()
}

fn from_flags(args: &InitArgs) -> cli_engine::Result<Template> {
    let field = |value: &Option<String>, flag: &str| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| missing_identity(flag))
    };
    let provider_id = field(&args.provider_id, "provider-id")?;
    let provider_name = field(&args.provider_name, "provider-name")?;
    let service_id = field(&args.service_id, "service-id")?;
    let service_name = field(&args.service_name, "service-name")?;
    check_provider_id(&provider_id).map_err(GddyError::into_cli_error)?;
    check_service_id(&service_id).map_err(GddyError::into_cli_error)?;
    if let Some(logo_url) = &args.logo_url {
        check_logo_url(logo_url).map_err(GddyError::into_cli_error)?;
    }

    let mut template = Template::from_basic_inputs(
        provider_id,
        provider_name,
        service_id,
        service_name,
        args.description.clone(),
    );
    template.logo_url = args.logo_url.clone();
    template.variable_description = args.variable_description.clone();
    template.sync_pub_key_domain = args.sync_pub_key_domain.clone();
    template.sync_redirect_domain = args.sync_redirect_domain.clone();
    template.sync_block = args.sync_block;
    template.shared = args.shared;
    template.shared_provider_name = args.shared_provider_name;
    template.shared_service_name = args.shared_service_name;
    template.multi_instance = args.multi_instance;
    template.warn_phishing = args.warn_phishing;
    template.host_required = args.host_required;

    let by_kind = [
        (RecordKind::A, &args.add_a),
        (RecordKind::Aaaa, &args.add_aaaa),
        (RecordKind::Cname, &args.add_cname),
        (RecordKind::Mx, &args.add_mx),
        (RecordKind::Txt, &args.add_txt),
        (RecordKind::Srv, &args.add_srv),
        (RecordKind::Ns, &args.add_ns),
        (RecordKind::Spfm, &args.add_spfm),
        (RecordKind::Redir301, &args.add_redir301),
        (RecordKind::Redir302, &args.add_redir302),
        (RecordKind::Apexcname, &args.add_apexcname),
        (RecordKind::Caa, &args.add_caa),
    ];
    for (kind, inputs) in by_kind {
        template.records.extend(parse_records(
            kind,
            inputs,
            args.group_id.as_deref(),
            args.essential,
        )?);
    }
    Ok(template)
}

fn needs_terminal() -> cli_engine::CliCoreError {
    GddyError::validation(
        "This command asks questions, and there is no terminal to answer them in.",
    )
    .with_fix(
        "Pass --no-interactive with --provider-id, --provider-name, --service-id and \
         --service-name, plus an --add-<type> flag for each DNS record.",
    )
    .with_system(SYSTEM)
    .into_cli_error()
}

fn file_name(template: &Template) -> String {
    format!("{}.{}.json", template.provider_id, template.service_id)
}

/// What a run reports, or the error it stops with, given whether the file
/// exists. Shared by the real and dry-run paths so they agree.
fn write_action(path: &Path, existed: bool, force: bool) -> cli_engine::Result<&'static str> {
    if existed && !force {
        return Err(
            GddyError::validation(format!("{} already exists.", path.display()))
                .with_fix("Pass --force to replace it, or move the existing file first.")
                .with_system(SYSTEM)
                .into_cli_error(),
        );
    }
    Ok(if existed { "overwritten" } else { "created" })
}

fn view_columns() -> Vec<TableColumn> {
    vec![
        TableColumn::new("file", "File").no_truncate(true),
        TableColumn::new("action", "Action"),
        TableColumn::new("providerId", "Provider"),
        TableColumn::new("serviceId", "Service"),
        TableColumn::new("recordCount", "Records"),
    ]
}

fn next_actions(file: &str) -> Vec<NextAction> {
    vec![
        next_action(
            "domain-connect template validate <file>",
            "Check the template for problems before submitting it",
        )
        .with_param("file", NextActionParam::value(file)),
    ]
}

/// Writes the template into `dir` and builds the command result.
fn write_template(
    template: &Template,
    dir: &Path,
    force: bool,
    dry_run: bool,
) -> cli_engine::Result<CommandResult> {
    check_provider_id(&template.provider_id).map_err(GddyError::into_cli_error)?;
    check_service_id(&template.service_id).map_err(GddyError::into_cli_error)?;
    let path = dir.join(file_name(template));
    let existed = path.exists();
    let action = write_action(&path, existed, force)?;
    let contents = template.to_json_pretty().map_err(|error| {
        GddyError::unexpected(format!("could not encode the template: {error}"))
            .with_system(SYSTEM)
            .into_cli_error()
    })?;
    let file = path.display().to_string();
    let data = |action: &str| {
        json!({
            "file": file,
            "action": action,
            "providerId": template.provider_id,
            "serviceId": template.service_id,
            "recordCount": template.records.len(),
            "template": template,
        })
    };
    if dry_run {
        let preview = if existed {
            "would overwrite"
        } else {
            "would create"
        };
        return Ok(CommandResult::new(data(preview)).with_dry_run());
    }
    std::fs::write(&path, format!("{contents}\n")).map_err(|error| {
        GddyError::unexpected(format!("could not write {file}: {error}"))
            .with_fix("Check that you can write to this directory, then run the command again.")
            .with_system(SYSTEM)
            .into_cli_error()
    })?;
    Ok(CommandResult::new(data(action)).with_next_actions(next_actions(&file)))
}

async fn run(ctx: CommandContext, args: InitArgs) -> cli_engine::Result<CommandResult> {
    let template = if args.no_interactive {
        from_flags(&args)?
    } else {
        let stdin = std::io::stdin();
        if !stdin.is_terminal() {
            return Err(needs_terminal());
        }
        prompt_template(&mut stdin.lock(), &mut std::io::stderr())?
    };
    let dir = std::env::current_dir().map_err(|error| {
        GddyError::unexpected(format!("could not find the current directory: {error}"))
            .with_system(SYSTEM)
            .into_cli_error()
    })?;
    write_template(&template, &dir, args.force, ctx.dry_run())
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<InitArgs, _, _, _>(
        CommandSpec::from_args::<InitArgs>("init", "Create a Domain Connect template file")
            .with_long(
                "Write a Domain Connect template to <provider-id>.<service-id>.json in the \
                 current directory. A template lists the DNS records your service needs, so \
                 customers can connect their domain to it in one step.\n\
                 \n\
                 In a terminal, the command asks for each value. With --no-interactive it \
                 builds the template from flags instead. An existing file is left alone \
                 unless you pass --force.\n\
                 \n\
                 Each --add-<type> flag takes key=value pairs separated by commas. An \
                 unknown key is rejected and the error lists the keys that record type \
                 accepts. In an A or AAAA record, value is either a literal address or a \
                 variable name: \"value=ip\" is filled in when the template is applied.\n\
                 \n\
                 Example:\n\
                   gddy domain-connect template init \\\n\
                     --no-interactive \\\n\
                     --provider-id acme.com \\\n\
                     --provider-name \"Acme Corp\" \\\n\
                     --service-id email \\\n\
                     --service-name \"Acme Email\" \\\n\
                     --add-mx \"value=mail.acme.com,priority=10\" \\\n\
                     --add-txt \"data=v=spf1 include:_spf.acme.com ~all\"\n\
                 \n\
                 Next, check the file with `gddy domain-connect template validate`.\n\
                 \n\
                 AI assistants: use --no-interactive; the prompts need a terminal. Ask the \
                 user for the provider and service details instead of inventing them.",
            )
            .with_system(SYSTEM)
            .with_tier(Tier::Mutate)
            .mutates(true)
            .handles_dry_run(true)
            .no_auth(true)
            .with_output_schema::<InitOutput>()
            .with_view(view_columns()),
        run,
    )
}

#[cfg(test)]
#[path = "init_tests.rs"]
mod tests;
