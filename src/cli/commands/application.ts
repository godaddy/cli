import { Command } from "commander";
import {
	type Application,
	type ApplicationInfo,
	type CreateApplicationInput,
	type CreateReleaseInput,
	type CreatedApplicationInfo,
	type DeployResult,
	type ExtensionSecurityReport,
	type ReleaseInfo,
	type ValidationResult,
	applicationArchive,
	applicationDeploy,
	applicationDisable,
	applicationEnable,
	applicationInfo,
	applicationInit,
	applicationList,
	applicationRelease,
	applicationUpdate,
	applicationValidate,
} from "../../core/applications";
import { type Environment, envGet } from "../../core/environment";
import type { Finding } from "../../core/security/types";
import {
	type ActionConfig,
	type BlocksExtensionConfig,
	type CheckoutExtensionConfig,
	type EmbedExtensionConfig,
	type SubscriptionConfig,
	addActionToConfig,
	addExtensionToConfig,
	addSubscriptionToConfig,
	getConfigFile,
} from "../../services/config";
import { getExtensions } from "../../services/extension/workspace";

async function getEnvironment(): Promise<Environment> {
	const result = await envGet();
	if (!result.success || !result.data) {
		throw result.error ?? new Error("Failed to get environment");
	}
	return result.data as Environment;
}

export function createApplicationCommand(): Command {
	const app = new Command("application")
		.alias("app")
		.description("Manage applications");

	// application info [name]
	app
		.command("info")
		.description("Show application information")
		.argument("[name]", "Application name")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (name, options) => {
			if (!name) {
				console.error("Application name is required");
				console.log("Usage: godaddy application info <name>");
				process.exit(1);
			}

			const result = await applicationInfo(name);

			if (!result.success) {
				console.error(
					result.error?.userMessage || "Failed to get application info",
				);
				process.exit(1);
			}

			const appInfo = result.data as ApplicationInfo;

			if (options.output === "json") {
				console.log(JSON.stringify(appInfo, null, 2));
			} else {
				console.log(`Application: ${appInfo.name}`);
				console.log(`  ID: ${appInfo.id}`);
				console.log(`  Label: ${appInfo.label}`);
				console.log(`  Description: ${appInfo.description}`);
				console.log(`  Status: ${appInfo.status}`);
				console.log(`  URL: ${appInfo.url}`);
				console.log(`  Proxy URL: ${appInfo.proxyUrl}`);

				if (appInfo.authorizationScopes?.length) {
					console.log(
						`  Authorization Scopes: ${appInfo.authorizationScopes.join(", ")}`,
					);
				}

				if (appInfo.releases?.length) {
					console.log(`  Latest Release: ${appInfo.releases[0].version}`);
					if (appInfo.releases[0].description) {
						console.log(`    Description: ${appInfo.releases[0].description}`);
					}
					console.log(
						`    Created: ${new Date(appInfo.releases[0].createdAt).toLocaleString()}`,
					);
				}
			}
			process.exit(0);
		});

	// application list
	app
		.command("list")
		.alias("ls")
		.description("List all applications")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (options) => {
			const result = await applicationList();

			if (!result.success) {
				console.error(
					result.error?.userMessage || "Failed to list applications",
				);
				process.exit(1);
			}

			const applications = result.data as Application[];

			if (options.output === "json") {
				console.log(JSON.stringify(applications, null, 2));
			} else {
				if (applications.length === 0) {
					console.log("No applications found");
					return;
				}

				console.log(`Found ${applications.length} application(s):`);
				for (const app of applications) {
					console.log(`  ${app.name} (${app.status})`);
					console.log(`    Label: ${app.label}`);
					console.log(`    Description: ${app.description}`);
					console.log(`    URL: ${app.url}`);
					console.log("");
				}
			}
			process.exit(0);
		});

	// application validate [name]
	app
		.command("validate")
		.description("Validate application configuration")
		.argument("[name]", "Application name")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (name, options) => {
			if (!name) {
				console.error("Application name is required");
				console.log("Usage: godaddy application validate <name>");
				process.exit(1);
			}

			const result = await applicationValidate(name);

			if (!result.success) {
				console.error(
					result.error?.userMessage || "Failed to validate application",
				);
				process.exit(1);
			}

			const validation = result.data as ValidationResult;

			if (options.output === "json") {
				console.log(JSON.stringify(validation, null, 2));
			} else {
				if (validation.valid) {
					console.log(`✅ Application '${name}' is valid`);
				} else {
					console.log(`❌ Application '${name}' has validation errors`);
				}

				if (validation.errors.length > 0) {
					console.log("\nErrors:");
					for (const error of validation.errors) {
						console.log(`  ❌ ${error}`);
					}
				}

				if (validation.warnings.length > 0) {
					console.log("\nWarnings:");
					for (const warning of validation.warnings) {
						console.log(`  ⚠️  ${warning}`);
					}
				}

				if (!validation.valid) {
					process.exit(1);
				}
			}
			process.exit(0);
		});

	// application update <name>
	app
		.command("update")
		.description("Update application configuration")
		.argument("<name>", "Application name")
		.option("--label <label>", "Application label")
		.option("--description <description>", "Application description")
		.option("--status <status>", "Application status (ACTIVE|INACTIVE)")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (name, options) => {
			const config: {
				label?: string;
				description?: string;
				status?: "ACTIVE" | "INACTIVE";
			} = {};

			if (options.label) config.label = options.label;
			if (options.description) config.description = options.description;
			if (options.status) {
				if (!["ACTIVE", "INACTIVE"].includes(options.status)) {
					console.error("Status must be either ACTIVE or INACTIVE");
					process.exit(1);
				}
				config.status = options.status;
			}

			if (Object.keys(config).length === 0) {
				console.error("At least one field must be specified for update");
				console.log("Available options: --label, --description, --status");
				process.exit(1);
			}

			const result = await applicationUpdate(name, config);

			if (!result.success) {
				console.error(
					result.error?.userMessage || "Failed to update application",
				);
				process.exit(1);
			}

			if (options.output === "json") {
				console.log(JSON.stringify({ success: true }, null, 2));
			} else {
				console.log(`✅ Successfully updated application '${name}'`);
			}
			process.exit(0);
		});

	// application enable <name>
	app
		.command("enable")
		.description("Enable application on a store")
		.argument("<name>", "Application name")
		.option("--store-id <storeId>", "Store ID")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (name, options) => {
			if (!options.storeId) {
				console.error("Store ID is required");
				console.log(
					"Usage: godaddy application enable <name> --store-id <storeId>",
				);
				process.exit(1);
			}

			const result = await applicationEnable(name, options.storeId);

			if (!result.success) {
				console.error(
					result.error?.userMessage || "Failed to enable application",
				);
				process.exit(1);
			}

			if (options.output === "json") {
				console.log(JSON.stringify({ success: true }, null, 2));
			} else {
				console.log(
					`✅ Successfully enabled application '${name}' on store ${options.storeId}`,
				);
			}
			process.exit(0);
		});

	// application disable <name>
	app
		.command("disable")
		.description("Disable application on a store")
		.argument("<name>", "Application name")
		.option("--store-id <storeId>", "Store ID")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (name, options) => {
			if (!options.storeId) {
				console.error("Store ID is required");
				console.log(
					"Usage: godaddy application disable <name> --store-id <storeId>",
				);
				process.exit(1);
			}

			const result = await applicationDisable(name, options.storeId);

			if (!result.success) {
				console.error(
					result.error?.userMessage || "Failed to disable application",
				);
				process.exit(1);
			}

			if (options.output === "json") {
				console.log(JSON.stringify({ success: true }, null, 2));
			} else {
				console.log(
					`✅ Successfully disabled application '${name}' on store ${options.storeId}`,
				);
			}
			process.exit(0);
		});

	// application archive <name>
	app
		.command("archive")
		.description("Archive application")
		.argument("<name>", "Application name")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (name, options) => {
			const result = await applicationArchive(name);

			if (!result.success) {
				console.error(
					result.error?.userMessage || "Failed to archive application",
				);
				process.exit(1);
			}

			if (options.output === "json") {
				console.log(JSON.stringify({ success: true }, null, 2));
			} else {
				console.log(`✅ Successfully archived application '${name}'`);
			}
			process.exit(0);
		});

	// application init
	app
		.command("init")
		.description("Initialize/create a new application")
		.option("--name <name>", "Application name")
		.option("--description <description>", "Application description")
		.option("--url <url>", "Application URL")
		.option("--proxy-url <proxyUrl>", "Proxy URL for API endpoints")
		.option(
			"--scopes <scopes>",
			"Authorization scopes (space-separated)",
			(value) => value.split(" "),
		)
		.option("-c, --config <path>", "Path to configuration file")
		.option("--environment <env>", "Environment (ote|prod)")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (options) => {
			// Load config file if specified
			let cfg: ReturnType<typeof getConfigFile> | undefined;
			try {
				if (options.config || options.environment) {
					const result = getConfigFile({
						configPath: options.config,
						env: options.environment,
					});
					if (typeof result === "object" && "problems" in result) {
						// Handle arktype validation errors
						console.error("Config file validation failed:");
						for (const problem of result.problems || []) {
							console.error(`  ${problem.summary}`);
						}
						process.exit(1);
					}
					cfg = result;
				}
			} catch (err) {
				console.error(err instanceof Error ? err.message : String(err));
				process.exit(1);
			}

			// Merge config file values with CLI options (CLI overrides config)
			const input: CreateApplicationInput = {
				name: options.name ?? cfg?.name ?? "",
				description: options.description ?? cfg?.description ?? "",
				url: options.url ?? cfg?.url ?? "",
				proxyUrl: options.proxyUrl ?? cfg?.proxy_url ?? "",
				authorizationScopes: options.scopes ?? cfg?.authorization_scopes ?? [],
			};

			// Validate required fields
			if (!input.name) {
				console.log("Application name is required");
				process.exit(1);
			}

			if (!input.description) {
				console.log("Application description is required");
				process.exit(1);
			}

			if (!input.url) {
				console.log("Application URL is required");
				process.exit(1);
			}

			if (!input.proxyUrl) {
				console.log("Proxy URL is required");
				process.exit(1);
			}

			if (
				!input.authorizationScopes ||
				input.authorizationScopes.length === 0
			) {
				console.log("Authorization scopes are required");
				process.exit(1);
			}

			const environment = await getEnvironment();
			const result = await applicationInit(input, environment);

			if (!result.success) {
				console.error(
					result.error?.userMessage || "Failed to create application",
				);
				process.exit(1);
			}

			const appData = result.data as CreatedApplicationInfo;

			if (options.output === "json") {
				console.log(JSON.stringify(appData, null, 2));
			} else {
				console.log("✅ Application created successfully!");
				console.log(`  Name: ${appData.name}`);
				console.log(`  ID: ${appData.id}`);
				console.log(`  Client ID: ${appData.clientId}`);
				console.log(`  Status: ${appData.status}`);
				console.log(`  URL: ${appData.url}`);
				console.log(`  Proxy URL: ${appData.proxyUrl}`);
				console.log(
					`  Authorization Scopes: ${appData.authorizationScopes.join(", ")}`,
				);
				console.log("\n🔑 Credentials saved:");
				console.log("  GODADDY_CLIENT_ID");
				console.log("  GODADDY_CLIENT_SECRET");
				console.log("  GODADDY_WEBHOOK_SECRET");
				console.log("  GODADDY_PUBLIC_KEY");
			}
			process.exit(0);
		});

	// application add
	const addCommand = new Command("add").description(
		"Add configurations to application",
	);

	// application add action
	addCommand
		.command("action")
		.description("Add action configuration to godaddy.toml")
		.option("--name <name>", "Action name")
		.option("--url <url>", "Action endpoint URL")
		.option("--config <path>", "Path to configuration file")
		.option("--environment <env>", "Environment (ote|prod)")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (options) => {
			const actionName = options.name;
			const actionUrl = options.url;

			// Validate required fields
			if (!actionName) {
				console.error("Action name is required");
				console.log(
					"Usage: godaddy application add action --name <name> --url <url>",
				);
				process.exit(1);
			}

			if (!actionUrl) {
				console.error("Action URL is required");
				console.log(
					"Usage: godaddy application add action --name <name> --url <url>",
				);
				process.exit(1);
			}

			// Validate action name format
			if (actionName.length < 3) {
				console.error("Action name must be at least 3 characters long");
				process.exit(1);
			}

			const action: ActionConfig = {
				name: actionName,
				url: actionUrl,
			};

			try {
				const env = options.environment || (await getEnvironment());
				await addActionToConfig(action, {
					configPath: options.config,
					env: env,
				});

				if (options.output === "json") {
					console.log(JSON.stringify({ success: true, action }, null, 2));
				} else {
					console.log(`✅ Action '${actionName}' added successfully!`);
					console.log(`  Name: ${actionName}`);
					console.log(`  URL: ${actionUrl}`);
				}
			} catch (error) {
				console.error(
					error instanceof Error ? error.message : "Failed to add action",
				);
				process.exit(1);
			}
			process.exit(0);
		});

	// application add subscription
	addCommand
		.command("subscription")
		.description("Add webhook subscription configuration to godaddy.toml")
		.option("--name <name>", "Subscription name")
		.option("--events <events>", "Comma-separated list of events")
		.option("--url <url>", "Webhook endpoint URL")
		.option("--config <path>", "Path to configuration file")
		.option("--environment <env>", "Environment (ote|prod)")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (options) => {
			const subscriptionName = options.name;
			const events = options.events;
			const webhookUrl = options.url;

			// Validate required fields
			if (!subscriptionName) {
				console.error("Subscription name is required");
				console.log(
					"Usage: godaddy application add subscription --name <name> --events <events> --url <url>",
				);
				process.exit(1);
			}

			if (!events) {
				console.error("Events are required");
				console.log(
					"Usage: godaddy application add subscription --name <name> --events <events> --url <url>",
				);
				process.exit(1);
			}

			if (!webhookUrl) {
				console.error("Webhook URL is required");
				console.log(
					"Usage: godaddy application add subscription --name <name> --events <events> --url <url>",
				);
				process.exit(1);
			}

			// Validate subscription name format
			if (subscriptionName.length < 3) {
				console.error("Subscription name must be at least 3 characters long");
				process.exit(1);
			}

			// Parse events from comma-separated string
			const eventList = events
				.split(",")
				.map((e) => e.trim())
				.filter((e) => e.length > 0);
			if (eventList.length === 0) {
				console.error("At least one event is required");
				process.exit(1);
			}

			const subscription: SubscriptionConfig = {
				name: subscriptionName,
				events: eventList,
				url: webhookUrl,
			};

			try {
				const env = options.environment || (await getEnvironment());
				await addSubscriptionToConfig(subscription, {
					configPath: options.config,
					env: env,
				});

				if (options.output === "json") {
					console.log(JSON.stringify({ success: true, subscription }, null, 2));
				} else {
					console.log(
						`✅ Subscription '${subscriptionName}' added successfully!`,
					);
					console.log(`  Name: ${subscriptionName}`);
					console.log(`  Events: ${eventList.join(", ")}`);
					console.log(`  URL: ${webhookUrl}`);
				}
			} catch (error) {
				console.error(
					error instanceof Error ? error.message : "Failed to add subscription",
				);
				process.exit(1);
			}
			process.exit(0);
		});

	// application add extension (parent command for extension types)
	const extensionCommand = addCommand
		.command("extension")
		.description("Add UI extension configuration to godaddy.toml");

	// application add extension embed
	extensionCommand
		.command("embed")
		.description(
			"Add an embed extension (injected UI at specific page locations)",
		)
		.option("--name <name>", "Extension name")
		.option("--handle <handle>", "Extension handle (unique identifier)")
		.option("--source <source>", "Path to extension source file")
		.option(
			"--target <targets>",
			"Comma-separated list of target locations (e.g., body.end)",
		)
		.option("--config <path>", "Path to configuration file")
		.option("--environment <env>", "Environment (ote|prod)")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (options) => {
			const { name, handle, source, target } = options;

			if (!name) {
				console.error("Extension name is required");
				console.log(
					"Usage: godaddy application add extension embed --name <name> --handle <handle> --source <source> --target <targets>",
				);
				process.exit(1);
			}

			if (!handle) {
				console.error("Extension handle is required");
				console.log(
					"Usage: godaddy application add extension embed --name <name> --handle <handle> --source <source> --target <targets>",
				);
				process.exit(1);
			}

			if (!source) {
				console.error("Extension source is required");
				console.log(
					"Usage: godaddy application add extension embed --name <name> --handle <handle> --source <source> --target <targets>",
				);
				process.exit(1);
			}

			if (!target) {
				console.error("At least one target is required for embed extensions");
				console.log(
					"Usage: godaddy application add extension embed --name <name> --handle <handle> --source <source> --target <targets>",
				);
				process.exit(1);
			}

			if (name.length < 3) {
				console.error("Extension name must be at least 3 characters long");
				process.exit(1);
			}

			if (handle.length < 3) {
				console.error("Extension handle must be at least 3 characters long");
				process.exit(1);
			}

			const targets = target
				.split(",")
				.map((t: string) => t.trim())
				.filter((t: string) => t.length > 0)
				.map((t: string) => ({ target: t }));

			if (targets.length === 0) {
				console.error("At least one valid target is required");
				process.exit(1);
			}

			const extension: EmbedExtensionConfig = {
				name,
				handle,
				source,
				targets,
			};

			try {
				const env = options.environment || (await getEnvironment());
				await addExtensionToConfig("embed", extension, {
					configPath: options.config,
					env: env,
				});

				if (options.output === "json") {
					console.log(JSON.stringify({ success: true, extension }, null, 2));
				} else {
					console.log(`✅ Embed extension '${name}' added successfully!`);
					console.log(`  Name: ${name}`);
					console.log(`  Handle: ${handle}`);
					console.log(`  Source: ${source}`);
					console.log(
						`  Targets: ${targets.map((t: { target: string }) => t.target).join(", ")}`,
					);
				}
			} catch (error) {
				console.error(
					error instanceof Error ? error.message : "Failed to add extension",
				);
				process.exit(1);
			}
			process.exit(0);
		});

	// application add extension checkout
	extensionCommand
		.command("checkout")
		.description("Add a checkout extension (checkout flow UI)")
		.option("--name <name>", "Extension name")
		.option("--handle <handle>", "Extension handle (unique identifier)")
		.option("--source <source>", "Path to extension source file")
		.option(
			"--target <targets>",
			"Comma-separated list of checkout target locations",
		)
		.option("--config <path>", "Path to configuration file")
		.option("--environment <env>", "Environment (ote|prod)")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (options) => {
			const { name, handle, source, target } = options;

			if (!name) {
				console.error("Extension name is required");
				console.log(
					"Usage: godaddy application add extension checkout --name <name> --handle <handle> --source <source> --target <targets>",
				);
				process.exit(1);
			}

			if (!handle) {
				console.error("Extension handle is required");
				console.log(
					"Usage: godaddy application add extension checkout --name <name> --handle <handle> --source <source> --target <targets>",
				);
				process.exit(1);
			}

			if (!source) {
				console.error("Extension source is required");
				console.log(
					"Usage: godaddy application add extension checkout --name <name> --handle <handle> --source <source> --target <targets>",
				);
				process.exit(1);
			}

			if (!target) {
				console.error(
					"At least one target is required for checkout extensions",
				);
				console.log(
					"Usage: godaddy application add extension checkout --name <name> --handle <handle> --source <source> --target <targets>",
				);
				process.exit(1);
			}

			if (name.length < 3) {
				console.error("Extension name must be at least 3 characters long");
				process.exit(1);
			}

			if (handle.length < 3) {
				console.error("Extension handle must be at least 3 characters long");
				process.exit(1);
			}

			const targets = target
				.split(",")
				.map((t: string) => t.trim())
				.filter((t: string) => t.length > 0)
				.map((t: string) => ({ target: t }));

			if (targets.length === 0) {
				console.error("At least one valid target is required");
				process.exit(1);
			}

			const extension: CheckoutExtensionConfig = {
				name,
				handle,
				source,
				targets,
			};

			try {
				const env = options.environment || (await getEnvironment());
				await addExtensionToConfig("checkout", extension, {
					configPath: options.config,
					env: env,
				});

				if (options.output === "json") {
					console.log(JSON.stringify({ success: true, extension }, null, 2));
				} else {
					console.log(`✅ Checkout extension '${name}' added successfully!`);
					console.log(`  Name: ${name}`);
					console.log(`  Handle: ${handle}`);
					console.log(`  Source: ${source}`);
					console.log(
						`  Targets: ${targets.map((t: { target: string }) => t.target).join(", ")}`,
					);
				}
			} catch (error) {
				console.error(
					error instanceof Error ? error.message : "Failed to add extension",
				);
				process.exit(1);
			}
			process.exit(0);
		});

	// application add extension blocks
	extensionCommand
		.command("blocks")
		.description(
			"Set the blocks extension source (consolidated UI blocks package)",
		)
		.option("--source <source>", "Path to blocks extension source file")
		.option("--config <path>", "Path to configuration file")
		.option("--environment <env>", "Environment (ote|prod)")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (options) => {
			const { source } = options;

			if (!source) {
				console.error("Extension source is required");
				console.log(
					"Usage: godaddy application add extension blocks --source <source>",
				);
				process.exit(1);
			}

			const extension: BlocksExtensionConfig = {
				source,
			};

			try {
				const env = options.environment || (await getEnvironment());
				await addExtensionToConfig("blocks", extension, {
					configPath: options.config,
					env: env,
				});

				if (options.output === "json") {
					console.log(JSON.stringify({ success: true, extension }, null, 2));
				} else {
					console.log("✅ Blocks extension configured successfully!");
					console.log(`  Source: ${source}`);
				}
			} catch (error) {
				console.error(
					error instanceof Error ? error.message : "Failed to add extension",
				);
				process.exit(1);
			}
			process.exit(0);
		});

	app.addCommand(addCommand);

	// application release
	app
		.command("release")
		.description("Create a new release for the application")
		.argument("<name>", "Application name")
		.option("--release-version <version>", "Release version")
		.option("--description <description>", "Release description")
		.option("--config <path>", "Path to configuration file")
		.option("--environment <env>", "Environment (ote|prod)")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (name, options) => {
			// Validate required fields
			if (!options.releaseVersion) {
				console.error("Release version is required");
				console.log(
					"Usage: godaddy application release <name> --release-version <version>",
				);
				process.exit(1);
			}

			const env = options.environment || "ote"; // || (await getEnvironment());
			const input = {
				// : CreateReleaseInput
				applicationName: name,
				version: options.releaseVersion,
				description: options.description,
				configPath: options.config,
				env: env,
			};

			const result = await applicationRelease(input);

			if (!result.success) {
				console.error(result.error?.userMessage || "Failed to create release");
				process.exit(1);
			}

			const releaseInfo = result.data as ReleaseInfo;

			if (options.output === "json") {
				console.log(JSON.stringify(releaseInfo, null, 2));
			} else {
				console.log("✅ Release created successfully!");
				console.log(`  ID: ${releaseInfo.id}`);
				console.log(`  Version: ${releaseInfo.version}`);
				if (releaseInfo.description) {
					console.log(`  Description: ${releaseInfo.description}`);
				}
				console.log(
					`  Created: ${new Date(releaseInfo.createdAt).toLocaleString()}`,
				);
			}
			process.exit(0);
		});

	// application deploy
	app
		.command("deploy")
		.description("Deploy application (change status to ACTIVE)")
		.argument("<name>", "Application name")
		.option("--config <path>", "Path to configuration file")
		.option("--environment <env>", "Environment (ote|prod)")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (name, options) => {
			const env = options.environment || (await getEnvironment());
			const result = await applicationDeploy(name, {
				configPath: options.config,
				env,
			});

			if (!result.success) {
				console.error(
					result.error?.userMessage || "Failed to deploy application",
				);
				process.exit(1);
			}

			if (options.output === "json") {
				console.log(JSON.stringify({ success: true, ...result.data }, null, 2));
			} else {
				console.log(`✅ Application '${name}' deployed successfully!`);
				console.log("  Status changed to ACTIVE");
				if (result.data) {
					console.log(`  Scanned ${result.data.totalExtensions} extension(s)`);
					const totalFiles = result.data.securityReports.reduce(
						(sum, r) => sum + r.scannedFiles,
						0,
					);
					console.log(`  Total files scanned: ${totalFiles}`);

					// Show bundle information
					if (result.data.bundleReports?.length > 0) {
						console.log(
							`  Bundled ${result.data.bundleReports.length} extension(s):`,
						);
						for (const bundle of result.data.bundleReports) {
							const targetsInfo = bundle.targets?.length
								? ` → ${bundle.targets.join(", ")}`
								: "";
							console.log(
								`    - ${bundle.extensionName}: ${bundle.artifactName} (${(bundle.size / 1024).toFixed(2)} KB)${targetsInfo}`,
							);
						}
					}
				}
			}
			process.exit(0);
		});

	return app;
}
