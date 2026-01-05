#!/usr/bin/env node

import { Command } from "commander";
import packageJson from "../package.json";
import { createAuthCommand, createEnvCommand } from "./cli";
import { createActionsCommand } from "./cli/commands/actions";
import { createApplicationCommand } from "./cli/commands/application";
import { createWebhookCommand } from "./cli/commands/webhook";
import { validateEnvironment } from "./core/environment";
import { setDebugMode } from "./services/logger";

/**
 * Traditional CLI entry point using commander.js
 * This handles all subcommand-based interactions
 */
export function createCliProgram(): Command {
	const program = new Command();

	program
		.name("godaddy")
		.description(
			"GoDaddy Developer Platform CLI - Tools for building, managing, and integrating with GoDaddy's platform services",
		)
		.version(packageJson.version)
		.option(
			"-e, --env <environment>",
			"Set the target environment for commands (ote, prod)",
		)
		.option("--debug", "Enable debug logging for HTTP requests and responses")
		.addHelpText(
			"after",
			`
Environment Overview:
  ote  - Pre-production environment that mirrors production
  prod - Production environment for live applications

Example Usage:
  $ godaddy application info --env ote
  $ godaddy env set prod
  $ godaddy webhook events`,
		);

	// Global pre-action hook to validate environment option and set debug mode
	program.hook("preAction", async (thisCommand, actionCommand) => {
		const options = thisCommand.opts();

		// Enable debug logging if --debug flag is set
		if (options.debug) {
			setDebugMode(true);
		}

		if (options.env) {
			try {
				validateEnvironment(options.env);
			} catch (error) {
				console.error(`Error: ${(error as Error).message}`);
				process.exit(1);
			}
		}
	});

	// Add CLI commands
	program.addCommand(createEnvCommand());
	program.addCommand(createAuthCommand());
	program.addCommand(createActionsCommand());
	program.addCommand(createApplicationCommand());
	program.addCommand(createWebhookCommand());

	return program;
}
