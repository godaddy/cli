/**
 * Shared utility functions
 */

import type { CliError, CmdResult } from "./types";

/**
 * Create a successful command result
 */
export function createSuccessResult<T>(data: T): CmdResult<T> {
	return {
		success: true,
		data,
	};
}

/**
 * Create an error command result
 */
export function createErrorResult(error: CliError): CmdResult<never> {
	return {
		success: false,
		error,
	};
}

/**
 * Type guard to check if a result is successful
 */
export function isSuccessResult<T>(
	result: CmdResult<T>,
): result is CmdResult<T> & { success: true; data: T } {
	return result.success;
}

/**
 * Type guard to check if a result is an error
 */
export function isErrorResult<T>(
	result: CmdResult<T>,
): result is CmdResult<T> & { success: false; error: CliError } {
	return !result.success;
}
