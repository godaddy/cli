import { Command } from "commander";
import {
	type Environment,
	type EnvironmentInfo,
	envGet,
	envInfo,
	envList,
	envSet,
	getEnvironmentDisplay,
} from "../../core/environment";

export function createEnvCommand(): Command {
	const env = new Command("env").description(
		"Manage GoDaddy environments (ote, prod)",
	);

	// env list
	env
		.command("list")
		.description("List all available environments")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (options) => {
			const result = await envList();

			if (!result.success) {
				console.error(
					result.error?.userMessage || "Failed to list environments",
				);
				process.exit(1);
			}

			if (options.output === "json") {
				console.log(JSON.stringify(result.data, null, 2));
			} else {
				console.log("Available Environments:");
				result.data?.forEach((environment, index) => {
					const display = getEnvironmentDisplay(environment);
					const marker = index === 0 ? "● " : "○ ";
					const activeText = index === 0 ? " (active)" : "";
					console.log(`  ${marker}${display.label}${activeText}`);
				});
			}
			process.exit(0);
		});

	// env get
	env
		.command("get")
		.description("Get current active environment")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (options) => {
			const result = await envGet();

			if (!result.success) {
				console.error(result.error?.userMessage || "Failed to get environment");
				process.exit(1);
			}

			if (options.output === "json") {
				console.log(JSON.stringify({ environment: result.data }, null, 2));
			} else {
				const display = getEnvironmentDisplay(result.data as Environment);
				console.log(`Current active environment: ${display.label}`);
			}
			process.exit(0);
		});

	// env set
	env
		.command("set")
		.description("Set active environment")
		.argument("<environment>", "Environment to set (ote|prod)")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (environment: string, options) => {
			const result = await envSet(environment);

			if (!result.success) {
				console.error(result.error?.userMessage || "Failed to set environment");
				process.exit(1);
			}

			if (options.output === "json") {
				console.log(JSON.stringify({ success: true, environment }, null, 2));
			} else {
				const display = getEnvironmentDisplay(environment as Environment);
				console.log(`Environment successfully set to ${display.label}`);
			}
			process.exit(0);
		});

	// env info
	env
		.command("info")
		.description("Show detailed information about an environment")
		.argument(
			"[environment]",
			"Environment to show info for (defaults to current)",
		)
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (environment: string | undefined, options) => {
			const result = await envInfo(environment);

			if (!result.success) {
				console.error(
					result.error?.userMessage || "Failed to get environment info",
				);
				process.exit(1);
			}

			const info = result.data as EnvironmentInfo;

			if (options.output === "json") {
				console.log(JSON.stringify(info, null, 2));
			} else {
				console.log(`Environment: ${info.display.label}`);

				if (info.config) {
					console.log("\nConfiguration:");
					console.log(`  Name: ${info.config.name}`);
					console.log(`  Client ID: ${info.config.client_id}`);
					console.log(`  Version: ${info.config.version}`);
					console.log(`  URL: ${info.config.url}`);
					console.log(`  Proxy URL: ${info.config.proxy_url}`);
					console.log(
						`  Auth Scopes: ${info.config.authorization_scopes.join(", ")}`,
					);
					console.log(`\nConfig File: ${info.configFile}`);
				} else {
					console.log("\nNo configuration file found for this environment.");
					console.log(`Create one with: godaddy env init ${info.environment}`);
				}
			}
			process.exit(0);
		});

	return env;
}
