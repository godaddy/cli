/**
 * Shared HTTP helpers for API requests
 */

import { v7 as uuid } from "uuid";
import { type Environment, envGet, getApiUrl } from "../core/environment";

// Cached API base URL
let apiBaseUrl: string | null = null;

/**
 * Get or initialize the API base URL based on the environment
 */
export async function initApiBaseUrl(): Promise<string> {
	if (apiBaseUrl) return apiBaseUrl;

	// Use environment variable if set, otherwise determine from active environment
	if (process.env.APPLICATIONS_GRAPHQL_URL) {
		apiBaseUrl = process.env.APPLICATIONS_GRAPHQL_URL;
	} else {
		const result = await envGet();
		if (!result.success || !result.data) {
			throw result.error ?? new Error("Failed to get environment");
		}
		const env = result.data as Environment;
		apiBaseUrl = `${getApiUrl(env)}/v1/apps/app-registry-subgraph`;
	}

	return apiBaseUrl;
}

/**
 * Get standard request headers with authentication
 */
export function getRequestHeaders(accessToken: string): Record<string, string> {
	return {
		Authorization: `Bearer ${accessToken}`,
		"X-Request-ID": uuid(),
	};
}
