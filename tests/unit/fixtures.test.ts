import { describe, expect, test } from "vitest";
import {
	applicationFixtures,
	authFixtures,
	findApplicationByName,
	findApplicationWithReleases,
	webhookFixtures,
} from "../setup/fixtures/fixture-utils";

describe("Test Fixtures", () => {
	test("application fixtures have required fields", () => {
		const app = applicationFixtures.applications[0];
		expect(app).toHaveProperty("id");
		expect(app).toHaveProperty("name");
		expect(app).toHaveProperty("label");
		expect(app).toHaveProperty("status");
		expect(app).toHaveProperty("url");
	});

	test("webhook fixtures have required fields", () => {
		const eventType = webhookFixtures.eventTypes[0];
		expect(eventType).toHaveProperty("eventType");
		expect(eventType).toHaveProperty("description");
	});

	test("auth fixtures have required token fields", () => {
		const token = authFixtures.validTokenResponse;
		expect(token).toHaveProperty("access_token");
		expect(token).toHaveProperty("token_type", "Bearer");
		expect(token).toHaveProperty("expires_in");
	});

	test("finder utilities work correctly", () => {
		const app = findApplicationByName("test-app-1");
		expect(app).toBeTruthy();
		expect(app?.name).toBe("test-app-1");

		const appWithReleases = findApplicationWithReleases("test-app-1");
		expect(appWithReleases).toBeTruthy();
		expect(appWithReleases?.releases).toHaveLength(2);
	});
});
