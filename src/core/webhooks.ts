import { type } from "arktype";
import {
	type WebhookEventType,
	getWebhookEventsTypes,
} from "../services/webhook-events";
import {
	AuthenticationError,
	type CmdResult,
	NetworkError,
} from "../shared/types";
import { getFromKeychain } from "./auth";

// Type definitions for core webhook functions
export interface WebhookEvent {
	eventType: string;
	description: string;
}

/**
 * Get list of available webhook event types
 */
export async function webhookEvents(): Promise<CmdResult<WebhookEvent[]>> {
	try {
		const accessToken = await getFromKeychain("token");
		if (!accessToken) {
			return {
				success: false,
				error: new AuthenticationError(
					"Not authenticated",
					"Please run 'godaddy auth login' first",
				),
			};
		}

		const result = await getWebhookEventsTypes({ accessToken });

		const events: WebhookEvent[] = result.events.map((event) => ({
			eventType: event.eventType,
			description: event.description,
		}));

		return {
			success: true,
			data: events,
		};
	} catch (error) {
		return {
			success: false,
			error: new NetworkError(
				`Failed to get webhook events: ${error}`,
				error as Error,
			),
		};
	}
}
