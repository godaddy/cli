import { type Environment, envGet, getApiUrl } from "../core/environment";
import { logHttpRequest, logHttpResponse } from "./logger";

export type WebhookEventType = {
	eventType: string;
	description: string;
};

export async function getWebhookEventsTypes({
	accessToken,
}: {
	accessToken: string | null;
}): Promise<{ events: Array<WebhookEventType> }> {
	if (!accessToken) {
		throw new Error("Access token is required");
	}

	// Get the current environment and build the API URL
	const result = await envGet();
	if (!result.success || !result.data) {
		throw result.error ?? new Error("Failed to get environment");
	}
	const env = result.data as Environment;
	const baseUrl = getApiUrl(env);

	const url = `${baseUrl}/v1/apis/webhook-event-types`;
	const headers = {
		Authorization: `Bearer ${accessToken}`,
		UserAgent: "@godaddy/cli",
	};

	const startTime = Date.now();

	// Log HTTP request
	logHttpRequest({
		method: "GET",
		url,
		headers,
	});

	const response = await fetch(url, { headers });
	const duration = Date.now() - startTime;

	const json = await response.json();

	// Log HTTP response
	logHttpResponse({
		method: "GET",
		url,
		status: response.status,
		statusText: response.statusText,
		headers: response.headers ? {} : undefined,
		body: json,
		duration,
	});

	if (!response.ok) {
		throw new Error(json.error || "Authentication failed");
	}

	return json;
}
