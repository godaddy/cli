import * as fs from "node:fs";
import { v7 as uuid } from "uuid";
import {
	AuthenticationError,
	type CmdResult,
	NetworkError,
	ValidationError,
} from "../shared/types";
import { getTokenInfo } from "./auth";
import { type Environment, envGet, getApiUrl } from "./environment";

// Minimum seconds before expiry to consider token valid for a request
const TOKEN_EXPIRY_BUFFER_SECONDS = 30;

export type HttpMethod = "GET" | "POST" | "PUT" | "PATCH" | "DELETE";

export interface ApiRequestOptions {
	endpoint: string;
	method?: HttpMethod;
	fields?: Record<string, string>;
	body?: string;
	headers?: Record<string, string>;
	debug?: boolean;
}

export interface ApiResponse {
	status: number;
	statusText: string;
	headers: Record<string, string>;
	data: unknown;
}

/**
 * Make an authenticated request to the GoDaddy API
 */
export async function apiRequest(
	options: ApiRequestOptions,
): Promise<CmdResult<ApiResponse>> {
	const {
		endpoint,
		method = "GET",
		fields,
		body,
		headers = {},
		debug,
	} = options;

	// Get access token with expiry info
	const tokenInfo = await getTokenInfo();
	if (!tokenInfo) {
		const error = new AuthenticationError("No valid access token found");
		error.userMessage = "Not authenticated. Run 'godaddy auth login' first.";
		return {
			success: false,
			error,
		};
	}

	// Check if token is about to expire
	if (tokenInfo.expiresInSeconds < TOKEN_EXPIRY_BUFFER_SECONDS) {
		const error = new AuthenticationError("Access token is about to expire");
		error.userMessage = `Token expires in ${tokenInfo.expiresInSeconds}s. Run 'godaddy auth login' to refresh.`;
		return {
			success: false,
			error,
		};
	}

	const accessToken = tokenInfo.accessToken;

	// Build URL
	const urlResult = await buildUrl(endpoint);
	if (!urlResult.success || !urlResult.data) {
		return {
			success: false,
			error:
				urlResult.error ||
				new ValidationError(
					"Failed to build URL",
					"Could not build request URL",
				),
		};
	}
	const url = urlResult.data;

	// Build headers
	const requestHeaders: Record<string, string> = {
		Authorization: `Bearer ${accessToken}`,
		"X-Request-ID": uuid(),
		...headers,
	};

	// Build body
	let requestBody: string | undefined;
	if (body) {
		requestBody = body;
		if (!requestHeaders["Content-Type"]) {
			requestHeaders["Content-Type"] = "application/json";
		}
	} else if (fields && Object.keys(fields).length > 0) {
		requestBody = JSON.stringify(fields);
		if (!requestHeaders["Content-Type"]) {
			requestHeaders["Content-Type"] = "application/json";
		}
	}

	if (debug) {
		console.error(`> ${method} ${url}`);
		for (const [key, value] of Object.entries(requestHeaders)) {
			const displayValue =
				key.toLowerCase() === "authorization" ? "Bearer [REDACTED]" : value;
			console.error(`> ${key}: ${displayValue}`);
		}
		if (requestBody) {
			console.error(`> Body: ${requestBody}`);
		}
		console.error("");
	}

	try {
		const response = await fetch(url, {
			method,
			headers: requestHeaders,
			body: requestBody,
		});

		// Parse response headers
		const responseHeaders: Record<string, string> = {};
		response.headers.forEach((value, key) => {
			responseHeaders[key] = value;
		});

		if (debug) {
			console.error(`< ${response.status} ${response.statusText}`);
			for (const [key, value] of Object.entries(responseHeaders)) {
				console.error(`< ${key}: ${value}`);
			}
			console.error("");
		}

		// Parse response body
		let data: unknown;
		const contentType = response.headers.get("content-type") || "";
		if (contentType.includes("application/json")) {
			const text = await response.text();
			if (text) {
				try {
					data = JSON.parse(text);
				} catch {
					data = text;
				}
			}
		} else {
			data = await response.text();
		}

		// Check for error status codes
		if (!response.ok) {
			const errorMessage =
				typeof data === "object" && data !== null
					? JSON.stringify(data)
					: String(data || response.statusText);

			// Handle 401 Unauthorized specifically - token may be revoked or invalid
			if (response.status === 401) {
				const error = new AuthenticationError(
					`Authentication failed (401): ${errorMessage}`,
				);
				error.userMessage =
					"Your session has expired or is invalid. Run 'godaddy auth login' to re-authenticate.";
				return {
					success: false,
					error,
				};
			}

			// Handle 403 Forbidden - insufficient permissions
			if (response.status === 403) {
				const error = new AuthenticationError(
					`Access denied (403): ${errorMessage}`,
				);
				error.userMessage =
					"You don't have permission to access this resource. Check your account permissions.";
				return {
					success: false,
					error,
				};
			}

			const error = new NetworkError(
				`API error (${response.status}): ${errorMessage}`,
			);
			return {
				success: false,
				error,
			};
		}

		return {
			success: true,
			data: {
				status: response.status,
				statusText: response.statusText,
				headers: responseHeaders,
				data,
			},
		};
	} catch (err) {
		const originalError = err instanceof Error ? err : new Error(String(err));
		return {
			success: false,
			error: new NetworkError("Network request failed", originalError),
		};
	}
}

/**
 * Build full URL from endpoint
 */
async function buildUrl(endpoint: string): Promise<CmdResult<string>> {
	// Reject full URLs - only relative paths are allowed
	if (endpoint.startsWith("http://") || endpoint.startsWith("https://")) {
		return {
			success: false,
			error: new ValidationError(
				"Full URLs are not allowed",
				"Only relative endpoints are allowed (e.g., /v1/domains). Full URLs are not permitted.",
			),
		};
	}

	// Get base URL from environment
	const envResult = await envGet();
	if (!envResult.success || !envResult.data) {
		return {
			success: false,
			error:
				envResult.error ||
				new ValidationError(
					"Failed to get environment",
					"Could not determine environment. Run 'godaddy env set <env>' first.",
				),
		};
	}
	const env = envResult.data as Environment;
	const baseUrl = getApiUrl(env);

	// Ensure endpoint starts with /
	const normalizedEndpoint = endpoint.startsWith("/")
		? endpoint
		: `/${endpoint}`;

	return { success: true, data: `${baseUrl}${normalizedEndpoint}` };
}

/**
 * Read JSON body from file
 */
export function readBodyFromFile(filePath: string): CmdResult<string> {
	try {
		if (!fs.existsSync(filePath)) {
			return {
				success: false,
				error: new ValidationError(
					`File not found: ${filePath}`,
					`File not found: ${filePath}`,
				),
			};
		}

		const content = fs.readFileSync(filePath, "utf-8");

		// Validate it's valid JSON
		try {
			JSON.parse(content);
		} catch {
			return {
				success: false,
				error: new ValidationError(
					`Invalid JSON in file: ${filePath}`,
					`File does not contain valid JSON: ${filePath}`,
				),
			};
		}

		return { success: true, data: content };
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error);
		return {
			success: false,
			error: new ValidationError(
				`Failed to read file: ${message}`,
				`Could not read file: ${filePath}`,
			),
		};
	}
}

/**
 * Parse field arguments into an object
 * Fields are in the format "key=value"
 */
export function parseFields(
	fields: string[],
): CmdResult<Record<string, string>> {
	const result: Record<string, string> = {};

	for (const field of fields) {
		const eqIndex = field.indexOf("=");
		if (eqIndex === -1) {
			return {
				success: false,
				error: new ValidationError(
					`Invalid field format: ${field}`,
					`Invalid field format: "${field}". Expected "key=value".`,
				),
			};
		}

		const key = field.slice(0, eqIndex);
		const value = field.slice(eqIndex + 1);

		if (!key) {
			return {
				success: false,
				error: new ValidationError(
					`Empty field key: ${field}`,
					`Empty field key in: "${field}"`,
				),
			};
		}

		result[key] = value;
	}

	return { success: true, data: result };
}

/**
 * Parse header arguments into an object
 * Headers are in the format "Key: Value"
 */
export function parseHeaders(
	headers: string[],
): CmdResult<Record<string, string>> {
	const result: Record<string, string> = {};

	for (const header of headers) {
		const colonIndex = header.indexOf(":");
		if (colonIndex === -1) {
			return {
				success: false,
				error: new ValidationError(
					`Invalid header format: ${header}`,
					`Invalid header format: "${header}". Expected "Key: Value".`,
				),
			};
		}

		const key = header.slice(0, colonIndex).trim();
		const value = header.slice(colonIndex + 1).trim();

		if (!key) {
			return {
				success: false,
				error: new ValidationError(
					`Empty header key: ${header}`,
					`Empty header key in: "${header}"`,
				),
			};
		}

		result[key] = value;
	}

	return { success: true, data: result };
}
