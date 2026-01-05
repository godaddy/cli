/**
 * Core type definitions shared across CLI and TUI
 */

// Generic result wrapper
export interface Result<T = unknown> {
	success: boolean;
	data?: T;
	error?: Error;
}

// Command result wrapper
export interface CmdResult<T = unknown> {
	success: boolean;
	data?: T;
	error?: CliError;
}

// Error type hierarchy
export abstract class CliError extends Error {
	abstract code: string;
	abstract userMessage: string;

	constructor(
		message: string,
		public originalError?: Error,
	) {
		super(message);
		this.name = this.constructor.name;
	}
}

export class ValidationError extends CliError {
	code = "VALIDATION_ERROR";
	userMessage: string;

	constructor(message: string, userMessage?: string) {
		super(message);
		this.userMessage = userMessage || message;
	}
}

export class NetworkError extends CliError {
	code = "NETWORK_ERROR";

	constructor(message: string, originalError?: Error) {
		super(message, originalError);
		let detail = "";
		if (
			originalError?.message &&
			originalError.message !== message &&
			!message.includes(originalError.message)
		) {
			detail = `: ${originalError.message}`;
		}
		this.userMessage = `Network error: ${message}${detail}`;
	}
}

export class AuthenticationError extends CliError {
	code = "AUTH_ERROR";
	userMessage = "Authentication failed";
}

export class ConfigurationError extends CliError {
	code = "CONFIG_ERROR";
	userMessage = "Configuration error";

	constructor(message: string, userMessage?: string) {
		super(message);
		this.userMessage = userMessage || message;
	}
}
