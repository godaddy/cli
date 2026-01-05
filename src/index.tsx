#!/usr/bin/env node

import { createCliProgram } from "./cli-entry";

/**
 * Main entry point for the GoDaddy CLI
 */
async function main(): Promise<void> {
	const program = createCliProgram();
	program.parse();
}

// Restore cursor visibility on exit or signals
const restoreCursor = () => {
	const terminal = process.stderr.isTTY
		? process.stderr
		: process.stdout.isTTY
			? process.stdout
			: undefined;
	terminal?.write("\u001B[?25h");
};

process.on("exit", restoreCursor);
process.on("SIGINT", () => {
	restoreCursor();
	process.exit(130);
});
process.on("SIGTERM", () => {
	restoreCursor();
	process.exit(143);
});

// Start the application
main().catch((error) => {
	console.error("Failed to start application:");
	console.error(error instanceof Error ? error.message : String(error));
	process.exit(1);
});
