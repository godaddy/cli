import * as fs from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import {
	type Config,
	getConfigFile,
	getConfigFilePath,
} from "../services/config";
import {
	type CmdResult,
	ConfigurationError,
	ValidationError,
} from "../shared/types";

export type Environment = "ote" | "prod";

export interface EnvironmentDisplay {
	color: string;
	label: string;
}

export interface EnvironmentInfo {
	environment: Environment;
	display: EnvironmentDisplay;
	configFile?: string;
	config?: Config;
}

const ENV_FILE = ".gdenv";
const ENV_PATH = join(homedir(), ENV_FILE);
const ALL_ENVIRONMENTS: Environment[] = ["ote", "prod"];

/**
 * Get all available environments
 */
export async function envList(): Promise<CmdResult<Environment[]>> {
	try {
		const activeEnv = await getActiveEnvironmentInternal();
		// Return environments with active one first
		const sorted = [
			activeEnv,
			...ALL_ENVIRONMENTS.filter((e) => e !== activeEnv),
		];
		return { success: true, data: sorted };
	} catch (error) {
		return {
			success: false,
			error: new ConfigurationError(
				`Failed to get environment list: ${error}`,
				"Could not retrieve environment list",
			),
		};
	}
}

/**
 * Get current active environment or specific environment info
 */
export async function envGet(
	name?: string,
): Promise<CmdResult<Environment | Environment[]>> {
	try {
		if (name) {
			const validEnv = validateEnvironment(name);
			return { success: true, data: validEnv };
		}

		const activeEnv = await getActiveEnvironmentInternal();
		return { success: true, data: activeEnv };
	} catch (error) {
		return {
			success: false,
			error:
				error instanceof ValidationError
					? error
					: new ConfigurationError(
							`Failed to get environment: ${error}`,
							"Could not retrieve environment information",
						),
		};
	}
}

/**
 * Set active environment
 */
export async function envSet(name: string): Promise<CmdResult<void>> {
	try {
		const validEnv = validateEnvironment(name);
		fs.writeFileSync(ENV_PATH, validEnv);
		return { success: true };
	} catch (error) {
		return {
			success: false,
			error:
				error instanceof ValidationError
					? error
					: new ConfigurationError(
							`Failed to set environment: ${error}`,
							"Could not set active environment",
						),
		};
	}
}

/**
 * Get detailed environment information
 */
export async function envInfo(
	name?: string,
): Promise<CmdResult<EnvironmentInfo>> {
	try {
		const env = name
			? validateEnvironment(name)
			: await getActiveEnvironmentInternal();
		const display = getEnvironmentDisplay(env);
		const configFilePath = getConfigFilePath(env);

		let config: Config | undefined;
		try {
			config = getConfigFile({ env });
		} catch {
			// Config file doesn't exist, which is fine
		}

		return {
			success: true,
			data: {
				environment: env,
				display,
				configFile: configFilePath,
				config,
			},
		};
	} catch (error) {
		return {
			success: false,
			error:
				error instanceof ValidationError
					? error
					: new ConfigurationError(
							`Failed to get environment info: ${error}`,
							"Could not retrieve environment information",
						),
		};
	}
}

/**
 * Get the current active environment (internal helper)
 */
async function getActiveEnvironmentInternal(): Promise<Environment> {
	try {
		if (fs.existsSync(ENV_PATH)) {
			const file = fs.readFileSync(ENV_PATH, "utf-8");
			const env = file.trim();
			return validateEnvironment(env);
		}
		return "ote";
	} catch (error) {
		return "ote";
	}
}

/**
 * Validate that the provided string is a valid environment
 */
export function validateEnvironment(env: string): Environment {
	const normalizedEnv = env.toLowerCase().trim();

	if (ALL_ENVIRONMENTS.includes(normalizedEnv as Environment)) {
		return normalizedEnv as Environment;
	}

	throw new ValidationError(
		`Invalid environment: ${env}. Must be one of: ${ALL_ENVIRONMENTS.join(", ")}`,
		`Invalid environment: ${env}. Must be one of: ${ALL_ENVIRONMENTS.join(", ")}`,
	);
}

/**
 * Get the display properties for an environment
 */
export function getEnvironmentDisplay(env: Environment): EnvironmentDisplay {
	const displays: Record<Environment, EnvironmentDisplay> = {
		ote: { color: "blue", label: "OTE" },
		prod: { color: "red", label: "PROD" },
	};

	return displays[env] || displays.ote;
}

/**
 * Generate the API URL for the given environment.
 * Can be overridden with GODADDY_API_BASE_URL environment variable.
 */
export function getApiUrl(env: Environment): string {
	if (process.env.GODADDY_API_BASE_URL) {
		return process.env.GODADDY_API_BASE_URL;
	}

	if (env === "prod") {
		return "https://api.godaddy.com";
	}
	return "https://api.ote-godaddy.com";
}

/**
 * Get the OAuth Client ID for the given environment.
 * Can be overridden with GODADDY_OAUTH_CLIENT_ID environment variable.
 */
export function getClientId(env: Environment): string {
	if (process.env.GODADDY_OAUTH_CLIENT_ID) {
		return process.env.GODADDY_OAUTH_CLIENT_ID;
	}

	const clientIds: Record<Environment, string> = {
		ote: "a502484b-d7b1-4509-aa88-08b391a54c28",
		prod: "39489dee-4103-4284-9aab-9f2452142bce",
	};

	return clientIds[env];
}

/**
 * Check if an action requires confirmation in the current environment
 */
export function requiresConfirmation(
	env: Environment,
	action: "deploy" | "release" | "delete" | "update",
): boolean {
	if (env === "prod") {
		return true;
	}

	if (env === "ote" && ["deploy", "release", "delete"].includes(action)) {
		return true;
	}

	return false;
}
