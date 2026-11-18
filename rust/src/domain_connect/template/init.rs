//! `gddy domain-connect template init` — Create a template interactively

use std::io::{BufRead, Write};

use cli_engine::{
    CommandResult, CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, Tier,
};
use serde_json::{Value, json};

use super::Template;
use super::interactive::{
    RecordType, prompt_a_record, prompt_aaaa_record, prompt_apexcname_record, prompt_caa_record,
    prompt_cname_record, prompt_mx_record, prompt_ns_record, prompt_optional_string,
    prompt_record_type, prompt_redir301_record, prompt_redir302_record, prompt_spfm_record,
    prompt_srv_record, prompt_string, prompt_txt_record,
};
use super::parsers::*;
use crate::error::GddyError;
use crate::next_action::next_action;
use crate::output_schema::output_schema;
use crate::scopes::{DOMAIN_CONNECT_TEMPLATE_READ, DOMAIN_CONNECT_TEMPLATE_WRITE};

output_schema!(InitOutput {
    "providerId": "string";
    "serviceId": "string";
    "fileName": "string";
    "recordCount": "number";
    "template": "object";
});

#[derive(Debug, Clone, clap::Args)]
struct InitArgs {
    /// Disable interactive prompts (requires all fields as flags)
    #[arg(long = "no-interactive", default_value_t = false)]
    no_interactive: bool,

    /// Provider domain (required in non-interactive mode, e.g., acme.com)
    #[arg(long = "provider-id", required_if_eq("no_interactive", "true"))]
    provider_id: Option<String>,

    /// Provider display name (required in non-interactive mode, e.g., "Acme Corp")
    #[arg(long = "provider-name", required_if_eq("no_interactive", "true"))]
    provider_name: Option<String>,

    /// Service identifier (required in non-interactive mode, e.g., "managed-wordpress")
    #[arg(long = "service-id", required_if_eq("no_interactive", "true"))]
    service_id: Option<String>,

    /// Service display name (required in non-interactive mode, e.g., "Acme Managed WordPress")
    #[arg(long = "service-name", required_if_eq("no_interactive", "true"))]
    service_name: Option<String>,

    /// Template description
    #[arg(long = "description")]
    description: Option<String>,

    /// Block synchronous setup until complete
    #[arg(long = "sync-block")]
    sync_block: Option<bool>,

    /// Template can be shared across providers
    #[arg(long = "shared")]
    shared: Option<bool>,

    /// Provider name is shared
    #[arg(long = "shared-provider-name")]
    shared_provider_name: Option<bool>,

    /// Service name is shared
    #[arg(long = "shared-service-name")]
    shared_service_name: Option<bool>,

    /// Allow multiple instances of this template
    #[arg(long = "multi-instance")]
    multi_instance: Option<bool>,

    /// Display phishing warning to user
    #[arg(long = "warn-phishing")]
    warn_phishing: Option<bool>,

    /// Host parameter is required from user
    #[arg(long = "host-required")]
    host_required: Option<bool>,

    /// Logo URL (must be http/https with jpg/png/svg extension)
    #[arg(long = "logo-url")]
    logo_url: Option<String>,

    /// Variable description for template parameters
    #[arg(long = "variable-description")]
    variable_description: Option<String>,

    /// Sync public key domain for verification
    #[arg(long = "sync-pub-key-domain")]
    sync_pub_key_domain: Option<String>,

    /// Sync redirect domain after setup
    #[arg(long = "sync-redirect-domain")]
    sync_redirect_domain: Option<String>,

    /// Add A record (repeatable). Format: "host=@,value=ip" or "host=www,value=%IP4%,ttl=3600,group=web"
    #[arg(long = "add-a")]
    add_a: Vec<String>,

    /// Add AAAA record (repeatable). Format: "host=@,value=IP6" or "host=www,value=%ipv6%,ttl=3600"
    #[arg(long = "add-aaaa")]
    add_aaaa: Vec<String>,

    /// Add CNAME record (repeatable). Format: "host=www,value=@" or "host=blog,value=host.provider.com,ttl=3600"
    #[arg(long = "add-cname")]
    add_cname: Vec<String>,

    /// Add MX record (repeatable). Format: "value=mail.example.com,priority=10" or "host=@,value=mail.com,priority=10,ttl=3600"
    #[arg(long = "add-mx")]
    add_mx: Vec<String>,

    /// Add TXT record (repeatable). Format: "host=@,data=v=spf1 ~all" or "host=_dmarc,data=v=DMARC1...,ttl=3600"
    #[arg(long = "add-txt")]
    add_txt: Vec<String>,

    /// Add SRV record (repeatable). Format: "service=_sip,protocol=_tcp,target=sip.example.com,port=5060,priority=100,weight=1"
    #[arg(long = "add-srv")]
    add_srv: Vec<String>,

    /// Add NS record (repeatable). Format: "host=@,value=ns1.example.com" or "host=subdomain,value=ns.provider.com,ttl=3600"
    #[arg(long = "add-ns")]
    add_ns: Vec<String>,

    /// Add SPFM record (repeatable). Format: "host=@,rules=include:_spf.example.com" or "rules=include:provider.com,ttl=3600"
    #[arg(long = "add-spfm")]
    add_spfm: Vec<String>,

    /// Add REDIR301 record (repeatable). Format: "host=@,target=https://www.%fqdn%" or "host=old,target=https://new.site.com"
    #[arg(long = "add-redir301")]
    add_redir301: Vec<String>,

    /// Add REDIR302 record (repeatable). Format: "host=@,target=https://www.%fqdn%" or "host=old,target=https://new.site.com"
    #[arg(long = "add-redir302")]
    add_redir302: Vec<String>,

    /// Add APEXCNAME record (repeatable). Format: "host=@,value=target.example.com" or "value=provider.com,ttl=3600"
    #[arg(long = "add-apexcname")]
    add_apexcname: Vec<String>,

    /// Add CAA record (repeatable). Format: "host=@,data=0 issue \"letsencrypt.org\"" or "data=0 issuewild \";\",ttl=3600"
    #[arg(long = "add-caa")]
    add_caa: Vec<String>,

    /// Global group ID for all records (can be overridden per-record with group=value)
    #[arg(long = "group-id")]
    group_id: Option<String>,

    /// Global essential value for all records (can be overridden per-record with essential=value)
    #[arg(long = "essential")]
    essential: Option<String>,
}

fn run_interactive() -> cli_engine::Result<Template> {
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let mut writer = std::io::stderr();

    writeln!(writer, "\n📝 Create Domain Connect Template\n")
        .map_err(|e| GddyError::unexpected(format!("Output error: {}", e)).into_cli_error())?;

    // Step 1: Basic template info
    let provider_id = loop {
        let input = prompt_string(&mut reader, &mut writer, "Provider ID (e.g., acme.com)")?;
        if input.contains('.') {
            break input;
        }
        writeln!(
            writer,
            "   ⚠ Provider ID must be a domain (e.g., acme.com)\n"
        )
        .map_err(|e| GddyError::unexpected(format!("Output error: {}", e)).into_cli_error())?;
    };

    let provider_name = prompt_string(&mut reader, &mut writer, "Provider Name")?;

    let service_id = loop {
        let input = prompt_string(&mut reader, &mut writer, "Service ID (e.g., email)")?;
        if input.chars().all(|c| c.is_alphanumeric() || c == '-') {
            break input;
        }
        writeln!(
            writer,
            "   ⚠ Service ID must be alphanumeric with hyphens only\n"
        )
        .map_err(|e| GddyError::unexpected(format!("Output error: {}", e)).into_cli_error())?;
    };

    let service_name = prompt_string(&mut reader, &mut writer, "Service Name")?;

    let description = prompt_optional_string(&mut reader, &mut writer, "Description (optional)")
        .map_err(|e| GddyError::unexpected(format!("Input error: {}", e)).into_cli_error())?;

    let mut template = Template::from_basic_inputs(
        provider_id,
        provider_name,
        service_id,
        service_name,
        description,
    );

    // Step 2: Add records
    writeln!(writer, "\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")
        .map_err(|e| GddyError::unexpected(format!("Output error: {}", e)).into_cli_error())?;
    writeln!(writer, "📋 Add DNS Records\n")
        .map_err(|e| GddyError::unexpected(format!("Output error: {}", e)).into_cli_error())?;

    loop {
        write!(writer, "Add a record? (y/N): ")
            .map_err(|e| GddyError::unexpected(format!("Output error: {}", e)).into_cli_error())?;
        writer
            .flush()
            .map_err(|e| GddyError::unexpected(format!("Output error: {}", e)).into_cli_error())?;

        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| GddyError::unexpected(format!("Input error: {}", e)).into_cli_error())?;

        let trimmed = line.trim().to_lowercase();
        if trimmed != "y" && trimmed != "yes" {
            break;
        }

        let record_type = prompt_record_type(&mut reader, &mut writer)?;

        let record = match record_type {
            RecordType::A => prompt_a_record(&mut reader, &mut writer)?,
            RecordType::AAAA => prompt_aaaa_record(&mut reader, &mut writer)?,
            RecordType::CNAME => prompt_cname_record(&mut reader, &mut writer)?,
            RecordType::MX => prompt_mx_record(&mut reader, &mut writer)?,
            RecordType::TXT => prompt_txt_record(&mut reader, &mut writer)?,
            RecordType::SRV => prompt_srv_record(&mut reader, &mut writer)?,
            RecordType::NS => prompt_ns_record(&mut reader, &mut writer)?,
            RecordType::SPFM => prompt_spfm_record(&mut reader, &mut writer)?,
            RecordType::REDIR301 => prompt_redir301_record(&mut reader, &mut writer)?,
            RecordType::REDIR302 => prompt_redir302_record(&mut reader, &mut writer)?,
            RecordType::APEXCNAME => prompt_apexcname_record(&mut reader, &mut writer)?,
            RecordType::CAA => prompt_caa_record(&mut reader, &mut writer)?,
        };

        template.records.push(record);
        writeln!(writer, "\n✅ Added {} record\n", record_type.as_str())
            .map_err(|e| GddyError::unexpected(format!("Output error: {}", e)).into_cli_error())?;
    }

    Ok(template)
}

fn init_next_actions(filename: &str) -> Vec<NextAction> {
    vec![
        next_action(
            "domain-connect template validate <file>",
            "Validate the generated template",
        )
        .with_param("file", NextActionParam::value(filename)),
        next_action(
            "domain-connect template submit <file>",
            "Submit the template to GoDaddy",
        )
        .with_param("file", NextActionParam::value(filename)),
    ]
}

fn human_init(_filename: &str, full_path: &str, record_count: usize, mut data: Value) -> Value {
    let summary_lines = [
        format!("Template saved: {}", full_path),
        format!(
            "Records: {}",
            if record_count == 1 {
                "1 record".to_string()
            } else {
                format!("{} records", record_count)
            }
        ),
    ];
    data["summary"] = Value::String(summary_lines.join("\n"));
    data
}

pub fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<InitArgs, _, _, _>(
        CommandSpec::from_args::<InitArgs>("init", "Create a new Domain Connect template")
            .with_long(
                "Create a Domain Connect template.\n\
                 \n\
                 By default, runs in interactive mode with prompts.\n\
                 Use --no-interactive with flags for scripting.\n\
                 \n\
                 Interactive example:\n\
                   gddy domain-connect template init\n\
                 \n\
                 Non-interactive example (multiple records with key=value format):\n\
                   gddy domain-connect template init \\\n\
                     --no-interactive \\\n\
                     --provider-id acme.com \\\n\
                     --provider-name \"Acme Corp\" \\\n\
                     --service-id email \\\n\
                     --service-name \"Email Service\" \\\n\
                     --description \"Email configuration\" \\\n\
                     --add-mx \"value=mail.acme.com,priority=10\" \\\n\
                     --add-mx \"value=mail2.acme.com,priority=20\" \\\n\
                     --add-txt \"data=v=spf1 include:_spf.acme.com ~all\" \\\n\
                     --add-cname \"host=dkim._domainkey,value=dkim.acme.com\"\n\
                 \n\
                 Record format: key=value pairs separated by commas.\n\
                 Common keys: host, value, data, priority, ttl, group, essential\n\
                 Example: --add-mx \"host=@,value=mail.com,priority=10,ttl=3600\"",
            )
            .with_system("domain-connect")
            .with_tier(Tier::Mutate)
            .with_scopes(&[DOMAIN_CONNECT_TEMPLATE_READ, DOMAIN_CONNECT_TEMPLATE_WRITE])
            .with_output_schema::<InitOutput>(),
        |ctx, args: InitArgs| async move {
            let template = if args.no_interactive {
                // Non-interactive mode: build from flags (clap validates required fields)
                let provider_id = args.provider_id.expect("clap validates required_if_eq");
                let provider_name = args.provider_name.expect("clap validates required_if_eq");
                let service_id = args.service_id.expect("clap validates required_if_eq");
                let service_name = args.service_name.expect("clap validates required_if_eq");

                // Validate provider ID (should be domain-like)
                if !provider_id.contains('.') {
                    return Err(GddyError::validation(
                        "Provider ID must be a domain (e.g., acme.com)",
                    )
                    .with_fix("Use a fully-qualified domain name for --provider-id")
                    .with_system("domain-connect")
                    .into_cli_error());
                }

                // Validate service ID (alphanumeric + hyphen)
                if !service_id.chars().all(|c| c.is_alphanumeric() || c == '-') {
                    return Err(GddyError::validation(
                        "Service ID must be alphanumeric with hyphens only",
                    )
                    .with_fix("Use only letters, numbers, and hyphens in --service-id")
                    .with_system("domain-connect")
                    .into_cli_error());
                }

                // Build template from flags
                let mut template = Template::from_basic_inputs(
                    provider_id.clone(),
                    provider_name.clone(),
                    service_id.clone(),
                    service_name.clone(),
                    args.description.clone(),
                );

                // Set additional template fields from args
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

                // Add records from flags using key=value parsers
                add_a_records(&mut template, &args.add_a, &args.group_id, &args.essential)?;
                add_aaaa_records(
                    &mut template,
                    &args.add_aaaa,
                    &args.group_id,
                    &args.essential,
                )?;
                add_cname_records(
                    &mut template,
                    &args.add_cname,
                    &args.group_id,
                    &args.essential,
                )?;
                add_mx_records(&mut template, &args.add_mx, &args.group_id, &args.essential)?;
                add_txt_records(
                    &mut template,
                    &args.add_txt,
                    &args.group_id,
                    &args.essential,
                )?;
                add_srv_records(
                    &mut template,
                    &args.add_srv,
                    &args.group_id,
                    &args.essential,
                )?;
                add_ns_records(&mut template, &args.add_ns, &args.group_id, &args.essential)?;
                add_spfm_records(
                    &mut template,
                    &args.add_spfm,
                    &args.group_id,
                    &args.essential,
                )?;
                add_redir301_records(
                    &mut template,
                    &args.add_redir301,
                    &args.group_id,
                    &args.essential,
                )?;
                add_redir302_records(
                    &mut template,
                    &args.add_redir302,
                    &args.group_id,
                    &args.essential,
                )?;
                add_apexcname_records(
                    &mut template,
                    &args.add_apexcname,
                    &args.group_id,
                    &args.essential,
                )?;
                add_caa_records(
                    &mut template,
                    &args.add_caa,
                    &args.group_id,
                    &args.essential,
                )?;

                template
            } else {
                // Interactive mode
                run_interactive()?
            };

            // Convert to JSON
            let json_output = template
                .to_json_pretty()
                .map_err(|e| GddyError::unexpected(e.to_string()).into_cli_error())?;

            // Generate filename from template
            let filename = format!("{}.{}.json", template.provider_id, template.service_id);

            // Get full path for display
            let full_path = std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join(&filename);

            // Write to file
            std::fs::write(&filename, &json_output).map_err(|e| {
                GddyError::unexpected(format!("Failed to write file {}: {}", filename, e))
                    .with_fix("Check file permissions and disk space")
                    .with_system("domain-connect")
                    .into_cli_error()
            })?;

            let data = json!({
                "providerId": template.provider_id,
                "serviceId": template.service_id,
                "fileName": filename,
                "recordCount": template.records.len(),
                "template": template,
            });

            let output = if ctx.middleware.output_format == "human" {
                human_init(
                    &filename,
                    &full_path.display().to_string(),
                    template.records.len(),
                    data,
                )
            } else {
                data
            };

            Ok(CommandResult::new(output).with_next_actions(init_next_actions(&filename)))
        },
    )
}

#[cfg(test)]
#[path = "init_tests.rs"]
mod tests;
