import * as fs from "node:fs";
import { join } from "node:path";
import * as TOML from "@iarna/toml";
import { type ArkErrors, type } from "arktype";
import type { Environment } from "../core/environment";
import type { CmdResult } from "../shared/types";

const Endpoint = type("string").narrow((endpoint: string, ctx) => {
	try {
		new URL(ctx.root?.proxy_url.concat(endpoint));
	} catch (error) {
		return ctx.mustBe("valid endpoint");
	}

	return true;
});

const SubscriptionConfig = type({
	name: type.string.atLeastLength(3),
	events: type.string.array().atLeastLength(1),
	url: Endpoint,
});

export type SubscriptionConfig = typeof SubscriptionConfig.infer;

const SubscriptionsType = type({
	webhook: SubscriptionConfig.array(),
});

export type SubscriptionsType = typeof SubscriptionsType.infer;

const ActionConfig = type({
	name: type.string.atLeastLength(3),
	url: Endpoint,
});

export type ActionConfig = typeof ActionConfig.infer;

const DependencyConfig = type({
	name: type.string.atLeastLength(3),
	version: type.keywords.string.semver.optional(),
});

export type DependencyConfig = typeof DependencyConfig.infer;

const DependenciesType = type({
	app: DependencyConfig.array().optional(),
	feature: DependencyConfig.array().optional(),
});

export type DependenciesType = typeof DependenciesType.infer;

const ExtensionTarget = type({
	target: type.string.atLeastLength(1),
});

export type ExtensionTarget = typeof ExtensionTarget.infer;

const EmbedExtensionConfig = type({
	name: type.string.atLeastLength(3),
	handle: type.string.atLeastLength(3),
	source: type.string.atLeastLength(1),
	targets: ExtensionTarget.array().atLeastLength(1),
});

export type EmbedExtensionConfig = typeof EmbedExtensionConfig.infer;

const CheckoutExtensionConfig = type({
	name: type.string.atLeastLength(3),
	handle: type.string.atLeastLength(3),
	source: type.string.atLeastLength(1),
	targets: ExtensionTarget.array().atLeastLength(1),
});

export type CheckoutExtensionConfig = typeof CheckoutExtensionConfig.infer;

const BlocksExtensionConfig = type({
	source: type.string.atLeastLength(1),
});

export type BlocksExtensionConfig = typeof BlocksExtensionConfig.infer;

const ExtensionsType = type({
	embed: EmbedExtensionConfig.array().optional(),
	checkout: CheckoutExtensionConfig.array().optional(),
	blocks: BlocksExtensionConfig.optional(),
});

export type ExtensionsType = typeof ExtensionsType.infer;

export type ExtensionType = "embed" | "checkout" | "blocks";

/**
 * Unified extension info extracted from config for deploy operations
 */
export interface ConfigExtensionInfo {
	/** Extension type (embed, checkout, block) */
	type: ExtensionType;
	/** Extension name */
	name: string;
	/** Extension handle (unique identifier) */
	handle: string;
	/** Path to extension source file (relative to repo root) */
	source: string;
	/** Optional targets for embed/checkout extensions */
	targets?: ExtensionTarget[];
}

/**
 * Extract all extensions from config file as a flat array.
 * This is the source of truth for what extensions should be scanned, bundled, and deployed.
 *
 * @param options - Config file options (configPath, env)
 * @returns Array of extension info objects, or empty array if no extensions defined
 */
export function getExtensionsFromConfig(
	options: { configPath?: string; env?: Environment } = {},
): ConfigExtensionInfo[] {
	const config = getConfigFile(options);

	if (typeof config !== "object" || "problems" in config) {
		return [];
	}

	const validConfig = config as Config;
	const extensions: ConfigExtensionInfo[] = [];

	if (validConfig.extensions?.embed) {
		for (const ext of validConfig.extensions.embed) {
			if (!ext.name || !ext.handle || !ext.source) {
				throw new Error(
					"Invalid embed extension config: missing required fields (name, handle, source)",
				);
			}
			extensions.push({
				type: "embed",
				name: ext.name,
				handle: ext.handle,
				source: ext.source,
				targets: ext.targets,
			});
		}
	}

	if (validConfig.extensions?.checkout) {
		for (const ext of validConfig.extensions.checkout) {
			if (!ext.name || !ext.handle || !ext.source) {
				throw new Error(
					"Invalid checkout extension config: missing required fields (name, handle, source)",
				);
			}
			extensions.push({
				type: "checkout",
				name: ext.name,
				handle: ext.handle,
				source: ext.source,
				targets: ext.targets,
			});
		}
	}

	if (validConfig.extensions?.blocks) {
		const blocks = validConfig.extensions.blocks;
		if (!blocks.source) {
			throw new Error(
				`Invalid blocks extension config: missing required 'source' field`,
			);
		}
		extensions.push({
			type: "blocks",
			name: "Blocks",
			handle: "blocks",
			source: blocks.source,
		});
	}

	return extensions;
}

const Config = type({
	name: "/^[a-z0-9-]{3,255}$/",
	client_id: type.keywords.string.uuid.v4,
	description: type.string.optional(),
	version: type.keywords.string.semver,
	url: type.keywords.string.url.root,
	proxy_url: type.keywords.string.url.root,
	authorization_scopes: type.string.array().moreThanLength(0),
	subscriptions: SubscriptionsType.optional(),
	actions: ActionConfig.array().optional(),
	dependencies: DependenciesType.array().optional(),
	extensions: ExtensionsType.optional(),
});

export type Config = typeof Config.infer;

/**
 * Get the configuration file path based on environment
 * @param env Optional environment to get config for
 * @param configPath Optional specific config path
 * @returns The resolved path to the config file
 */
export function getConfigFilePath(
	env?: Environment,
	configPath?: string,
): string {
	if (configPath) {
		return join(process.cwd(), configPath);
	}

	const fileName = env ? `godaddy.${env}.toml` : "godaddy.toml";
	return join(process.cwd(), fileName);
}

export function getConfigFile({
	configPath,
	env,
}: {
	configPath?: string;
	env?: Environment;
} = {}): Config | ArkErrors {
	// If a specific config path is provided, use that
	if (configPath) {
		const absolutePath = join(process.cwd(), configPath);

		if (fs.existsSync(absolutePath)) {
			const content = fs.readFileSync(absolutePath, "utf-8");
			return Config(TOML.parse(content));
		}

		throw new Error(`Config file not found at ${absolutePath}`);
	}

	// If no specific path is provided, try environment-specific file first
	if (env) {
		const envFilePath = getConfigFilePath(env);

		if (fs.existsSync(envFilePath)) {
			const content = fs.readFileSync(envFilePath, "utf-8");
			return Config(TOML.parse(content));
		}

		// Only show fallback message if we're not in prod environment
		if (env !== "prod") {
			console.warn(
				`No environment-specific config found for ${env}, falling back to default`,
			);
		}
	}

	// Fall back to default config file
	const defaultPath = getConfigFilePath();
	if (fs.existsSync(defaultPath)) {
		const content = fs.readFileSync(defaultPath, "utf-8");
		return Config(TOML.parse(content));
	}

	const envHint =
		env && env !== "prod"
			? ` Consider running 'godaddy application init' to create environment-specific configs.`
			: "";
	throw new Error(`Config file not found at ${defaultPath}.${envHint}`);
}

export async function createConfigFile(data: Config, env?: Environment) {
	const filePath = getConfigFilePath(env);
	const file = filePath;

	// Try to read the existing file to preserve structure
	let existingConfig = {};
	try {
		if (fs.existsSync(file)) {
			const existingContent = fs.readFileSync(file, "utf-8");
			existingConfig = TOML.parse(existingContent);
		}
	} catch (error) {
		// File doesn't exist or can't be parsed, use empty object
		console.log(`Creating new config file: ${filePath}`);
	}

	// Convert actions to the proper format
	const formattedActions = data.actions?.map((action) => {
		if (typeof action === "string") {
			// Convert string actions to object format
			return { name: action, url: "" };
		}
		return action;
	});

	// Create a new TOML object with the updated data
	// We need to use a type that's compatible with TOML.stringify
	const tomlData: Record<string, unknown> = {
		// Copy non-default properties from existing config that we want to preserve
		...Object.fromEntries(
			Object.entries(existingConfig as Record<string, unknown>).filter(
				([key]) =>
					![
						"name",
						"client_id",
						"description",
						"version",
						"url",
						"proxy_url",
						"authorization_scopes",
						"actions",
						"subscriptions",
						"default",
					].includes(key),
			),
		),
		// Update top-level properties
		name: data.name,
		client_id: data.client_id,
		description: data.description || "",
		version: data.version,
		url: data.url,
		proxy_url: data.proxy_url,
		authorization_scopes: data.authorization_scopes || [],
		// Update actions array
		actions: formattedActions,
		// Update subscriptions
		subscriptions: data.subscriptions,
	};

	// Preserve the default section if it exists
	if ("default" in existingConfig) {
		tomlData.default = (existingConfig as Record<string, unknown>).default;
	}

	// If dependencies or extensions exist, add them
	if (data.dependencies) {
		tomlData.dependencies = data.dependencies;
	}

	if (data.extensions) {
		tomlData.extensions = data.extensions;
	}

	// Remove undefined values before stringifying
	const cleanedTomlData = Object.entries(tomlData).reduce(
		(acc, [key, value]) => {
			if (value !== undefined) {
				acc[key] = value as TOML.AnyJson; // Assign known valid JSON types
			}
			return acc;
		},
		{} as TOML.JsonMap,
	);

	const tomlString = TOML.stringify(cleanedTomlData); // No need to cast here anymore
	fs.writeFileSync(filePath, tomlString);

	console.log(`Config file updated: ${filePath}`);
}

export async function updateVersionNumber(version: string | null) {
	if (!version) return;

	const config = getConfigFile();
	const newConfig = { ...config, version };
	await createConfigFile(newConfig);
}

/**
 * Determine which config file path to use for updates
 * Priority: explicit configPath > env-specific file > default file
 */
function getConfigFilePathForUpdate(
	configPath?: string,
	env?: Environment,
): { path: string; env?: Environment } {
	// If a specific config path is provided, use that
	if (configPath) {
		const absolutePath = join(process.cwd(), configPath);
		if (fs.existsSync(absolutePath)) {
			return { path: absolutePath };
		}
		throw new Error(`Config file not found at ${absolutePath}`);
	}

	// If env is provided, try environment-specific file first
	if (env) {
		const envFilePath = getConfigFilePath(env);
		if (fs.existsSync(envFilePath)) {
			return { path: envFilePath, env };
		}
	}

	// Fall back to default config file
	const defaultPath = getConfigFilePath();
	if (fs.existsSync(defaultPath)) {
		return { path: defaultPath };
	}

	// If no file exists, create environment-specific file if env is provided
	if (env) {
		return { path: getConfigFilePath(env), env };
	}

	return { path: defaultPath };
}

export async function addActionToConfig(
	action: ActionConfig,
	options: { configPath?: string; env?: Environment } = {},
): Promise<CmdResult<void>> {
	try {
		const config = getConfigFile(options);
		if (typeof config === "object" && "problems" in config) {
			throw new Error("Config file validation failed");
		}

		const updatedConfig = {
			...config,
			actions: [...(config.actions || []), action],
		};

		const { env } = getConfigFilePathForUpdate(options.configPath, options.env);
		await createConfigFile(updatedConfig, env);

		return { success: true, data: undefined };
	} catch (error) {
		return { success: false, error: error as Error };
	}
}

export async function addSubscriptionToConfig(
	subscription: SubscriptionConfig,
	options: { configPath?: string; env?: Environment } = {},
): Promise<CmdResult<void>> {
	try {
		const config = getConfigFile(options);
		if (typeof config === "object" && "problems" in config) {
			throw new Error("Config file validation failed");
		}

		const updatedConfig = {
			...config,
			subscriptions: {
				webhook: [...(config.subscriptions?.webhook || []), subscription],
			},
		};

		const { env } = getConfigFilePathForUpdate(options.configPath, options.env);
		await createConfigFile(updatedConfig, env);

		return { success: true, data: undefined };
	} catch (error) {
		return { success: false, error: error as Error };
	}
}

export async function createEnvFile(
	{
		secret,
		publicKey,
		clientId,
		clientSecret,
	}: {
		secret: string;
		publicKey: string;
		clientId: string;
		clientSecret: string;
	},
	env?: Environment,
) {
	const envFileName = env ? `.env.${env}` : ".env";
	const envPath = join(process.cwd(), envFileName);

	let envContent = "";
	try {
		if (fs.existsSync(envPath)) {
			const existingEnvContent = fs.readFileSync(envPath, "utf-8");

			// Parse existing .env file
			const envLines = existingEnvContent.split("\n");
			const envVars: Record<string, string> = {};

			// Extract existing environment variables
			for (const line of envLines) {
				if (line.trim() && !line.startsWith("#")) {
					const [key, ...valueParts] = line.split("=");
					if (key) {
						envVars[key.trim()] = valueParts.join("=").trim();
					}
				}
			}

			// Update with new values
			envVars.GODADDY_WEBHOOK_SECRET = secret;
			envVars.GODADDY_PUBLIC_KEY = publicKey;
			envVars.GODADDY_CLIENT_ID = clientId;
			envVars.GODADDY_CLIENT_SECRET = clientSecret;

			// Convert back to .env format
			envContent = Object.entries(envVars)
				.map(([key, value]) => `${key}=${value}`)
				.join("\n");

			// Preserve any comments or formatting by appending them if they're not associated with our keys
			for (const line of envLines) {
				if (line.trim() && (line.startsWith("#") || !line.includes("="))) {
					envContent += `\n${line}`;
				}
			}
		} else {
			// File doesn't exist, create new .env content
			envContent = `GODADDY_WEBHOOK_SECRET=${secret}\nGODADDY_PUBLIC_KEY=${publicKey}\nGODADDY_CLIENT_ID=${clientId}\nGODADDY_CLIENT_SECRET=${clientSecret}`;
		}
	} catch (error) {
		// Error reading file, create new .env content
		envContent = `GODADDY_WEBHOOK_SECRET=${secret}\nGODADDY_PUBLIC_KEY=${publicKey}\nGODADDY_CLIENT_ID=${clientId}\nGODADDY_CLIENT_SECRET=${clientSecret}`;
	}

	fs.writeFileSync(envPath, envContent);
	console.log(`Environment file updated: ${envFileName}`);
}

export async function addExtensionToConfig(
	extensionType: ExtensionType,
	extension:
		| EmbedExtensionConfig
		| CheckoutExtensionConfig
		| BlocksExtensionConfig,
	options: { configPath?: string; env?: Environment } = {},
): Promise<CmdResult<void>> {
	try {
		const config = getConfigFile(options);
		if (typeof config === "object" && "problems" in config) {
			throw new Error("Config file validation failed");
		}

		const currentExtensions = config.extensions || {};
		let updatedExtensions: ExtensionsType;

		if (extensionType === "blocks") {
			updatedExtensions = {
				...currentExtensions,
				blocks: extension as BlocksExtensionConfig,
			};
		} else {
			updatedExtensions = {
				...currentExtensions,
				[extensionType]: [
					...((currentExtensions[extensionType] as Array<unknown>) || []),
					extension,
				],
			} as ExtensionsType;
		}

		const updatedConfig = {
			...config,
			extensions: updatedExtensions,
		};

		const { env } = getConfigFilePathForUpdate(options.configPath, options.env);
		await createConfigFile(updatedConfig, env);

		return { success: true, data: undefined };
	} catch (error) {
		return { success: false, error: error as Error };
	}
}
