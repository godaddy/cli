import { describe, expect, it } from "vitest";
import { getWebhookEventsTypes } from "../../src/services/webhook-events";
import { webhookEventTypesFixture } from "../setup/fixtures/webhook-fixtures";
import { withNoAuth, withValidAuth } from "../setup/test-utils";

describe("webhook service", () => {
	describe("getWebhookEventsTypes", () => {
		it("should return webhook event types with valid auth", async () => {
			const result = await withValidAuth(async (accessToken) => {
				return getWebhookEventsTypes({ accessToken });
			});

			expect(result).toEqual(webhookEventTypesFixture);
			expect(result?.events).toHaveLength(8);
			expect(result?.events[0]).toEqual({
				eventType: "application.created",
				description: "Triggered when a new application is created",
			});
		});

		it("should throw error with null access token", async () => {
			await expect(
				getWebhookEventsTypes({ accessToken: null }),
			).rejects.toThrow("Access token is required");
		});

		it("should throw authentication error with invalid token", async () => {
			await withNoAuth(async () => {
				await expect(
					getWebhookEventsTypes({ accessToken: "invalid-token" }),
				).rejects.toThrow("Authentication failed");
			});
		});

		it("should return specific event types in response", async () => {
			const result = await withValidAuth(async (accessToken) => {
				return getWebhookEventsTypes({ accessToken });
			});

			const eventTypes = result?.events.map((event) => event.eventType);
			expect(eventTypes).toContain("application.created");
			expect(eventTypes).toContain("application.updated");
			expect(eventTypes).toContain("application.deleted");
			expect(eventTypes).toContain("application.enabled");
			expect(eventTypes).toContain("application.disabled");
			expect(eventTypes).toContain("application.archived");
			expect(eventTypes).toContain("release.created");
			expect(eventTypes).toContain("release.deployed");
		});

		it("should include descriptions for each event type", async () => {
			const result = await withValidAuth(async (accessToken) => {
				return getWebhookEventsTypes({ accessToken });
			});

			for (const event of result?.events || []) {
				expect(event.description).toBeTruthy();
				expect(typeof event.description).toBe("string");
				expect(event.description.length).toBeGreaterThan(0);
			}
		});
	});
});
