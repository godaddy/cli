//! `gddy domain-connect template submit` — Submit template (stubbed)

#![allow(clippy::print_stderr)]

use cli_engine::{CommandSpec, RuntimeCommandSpec};
use serde_json::json;

#[derive(Debug, Clone, clap::Args)]
struct SubmitArgs {
    /// Template file to submit
    #[arg(value_name = "FILE")]
    file: String,
}

pub fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<SubmitArgs, _, _, _>(
        CommandSpec::from_args::<SubmitArgs>("submit", "Submit a template to the registry")
            .with_long(
                "Submit a Domain Connect template to the public registry.\n\
                 \n\
                 This command is currently STUBBED and will:\n\
                 - Read the template file\n\
                 - Validate basic structure\n\
                 - Mock a backend API call\n\
                 - Print success message\n\
                 \n\
                 Example:\n\
                   gddy domain-connect template submit acme.com.wordpress.json",
            ),
        |_ctx, args: SubmitArgs| async move {
            // Convert to absolute path for display
            let file_path = std::path::Path::new(&args.file);
            let absolute_path = if file_path.is_absolute() {
                file_path.to_path_buf()
            } else {
                std::env::current_dir()
                    .unwrap_or_else(|_| std::path::PathBuf::from("."))
                    .join(file_path)
            };

            eprintln!("📤 Submitting template: {}", absolute_path.display());

            // Check if file exists
            if !file_path.exists() {
                return Err(cli_engine::CliCoreError::message(format!(
                    "File not found: {}\n\nMake sure you run this command from the directory containing the template file,\nor provide the full path to the file.",
                    absolute_path.display()
                )));
            }

            // Read file
            let content = std::fs::read_to_string(&args.file).map_err(|e| {
                cli_engine::CliCoreError::message(format!(
                    "Failed to read file {}: {}",
                    absolute_path.display(),
                    e
                ))
            })?;

            // Parse JSON to validate it's valid
            let template: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
                cli_engine::CliCoreError::message(format!("Invalid JSON in template: {}", e))
            })?;

            // Extract key fields for display
            let provider_id = template
                .get("providerId")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let service_id = template
                .get("serviceId")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            eprintln!("\n🔍 Validating template...");
            eprintln!("   Provider: {}", provider_id);
            eprintln!("   Service:  {}", service_id);

            // Simulate backend call (STUBBED)
            eprintln!("\n🌐 Calling backend API... (STUBBED)");
            eprintln!("   POST https://api.dev-godaddy.com/v1/domain-connect/templates");
            eprintln!("   {{ action: 'submit', template: {{...}} }}");

            // Simulate delay
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            // Mock success response
            eprintln!("\n✅ SUCCESS (Mock)");
            eprintln!("   Status: 201 Created");
            eprintln!("   PR URL: https://github.com/Domain-Connect/Templates/pull/123 (mock)");
            eprintln!("   Branch: add/{}.{} (mock)", provider_id, service_id);

            eprintln!("\n💡 This is a STUBBED response. Backend integration coming soon!");

            // Return structured result
            Ok(cli_engine::CommandResult::new(json!({
                "status": "success",
                "mock": true,
                "providerId": provider_id,
                "serviceId": service_id,
                "file": args.file,
                "prUrl": "https://github.com/Domain-Connect/Templates/pull/123",
                "message": "Template submitted successfully (mocked)",
            })))
        },
    )
}
