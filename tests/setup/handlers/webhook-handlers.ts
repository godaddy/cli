import { http, HttpResponse } from "msw";
import { checkAuthentication } from "../auth-utils";
import { webhookEventTypesFixture } from "../fixtures/webhook-fixtures";

/**
 * MSW handlers for webhook event types REST API endpoints
 * Handles endpoints at {env}-api.godaddy.com/v1/apis/webhook-event-types
 */

export const webhookHandlers = [
	// GET /v1/apis/webhook-event-types - List all webhook event types
	http.get("*/v1/apis/webhook-event-types", ({ request }) => {
		const authResult = checkAuthentication(request);
		if (authResult) {
			// authResult is an HttpResponse for unauthorized requests
			return authResult;
		}

		// authResult is null for valid authentication
		// Return the list of webhook event types
		return HttpResponse.json(webhookEventTypesFixture);
	}),
];
