import { beforeEach, describe, expect, test } from "vitest";
import {
	archiveApplication,
	createApplication,
	createRelease,
	disableApplication,
	enableApplication,
	getApplication,
	getApplicationAndLatestRelease,
	updateApplication,
} from "../../src/services/applications";
import { withValidAuth } from "../setup/test-utils";

describe("Application Service", () => {
	beforeEach(() => {
		withValidAuth();
	});

	test("gets application by name", async () => {
		const app = await getApplication("test-app-1", {
			accessToken: "test-token-123",
		});

		expect(app).toBeTruthy();
		expect(app?.application?.name).toBe("test-app-1");
		expect(app?.application?.status).toBe("ACTIVE");
	});

	test("returns null for non-existent application", async () => {
		const app = await getApplication("non-existent-app", {
			accessToken: "test-token-123",
		});
		expect(app.application).toBeNull();
	});

	test("gets application with releases", async () => {
		const app = await getApplicationAndLatestRelease("test-app-1", {
			accessToken: "test-token-123",
		});

		expect(app).toBeTruthy();
		expect(app?.application?.releases).toHaveLength(2);
	});

	test("creates new application", async () => {
		const input = {
			name: "new-test-app",
			label: "New Test Application",
			description: "A newly created test application",
			url: "https://new-test-app.example.com",
			proxyUrl: "https://proxy.new-test-app.example.com",
			authorizationScopes: ["read", "write"],
		};

		const app = await createApplication(input, {
			accessToken: "test-token-123",
		});

		expect(app).toBeTruthy();
		expect(app?.createApplication?.name).toBe(input.name);
		expect(app?.createApplication?.label).toBe(input.label);
		expect(app?.createApplication?.status).toBe("ACTIVE");
		expect(app?.createApplication?.clientId).toBeTruthy();
		expect(app?.createApplication?.clientSecret).toBeTruthy();
	});

	test("fails to create application with duplicate name", async () => {
		const input = {
			name: "test-app-1", // This already exists in fixtures
			label: "Duplicate App",
			description: "Duplicate application test",
			url: "https://duplicate.example.com",
			proxyUrl: "https://proxy.duplicate.example.com",
			authorizationScopes: ["read"],
		};

		await expect(
			createApplication(input, { accessToken: "test-token-123" }),
		).rejects.toThrow("already exists");
	});

	test("updates existing application", async () => {
		const updates = {
			label: "Updated Test Application",
			description: "This application has been updated",
		};

		const app = await updateApplication("app-1", updates, {
			accessToken: "test-token-123",
		});

		expect(app?.updateApplication?.label).toBe(updates.label);
		expect(app?.updateApplication?.description).toBe(updates.description);
		expect(app?.updateApplication?.id).toBe("app-1");
	});

	test("creates application release", async () => {
		const input = {
			applicationId: "app-1",
			version: "2.0.0",
			description: "Major version update",
			actions: [],
			subscriptions: [],
		};

		const release = await createRelease(input, {
			accessToken: "test-token-123",
		});

		expect(release?.createRelease?.version).toBe(input.version);
		expect(release?.createRelease?.description).toBe(input.description);
		expect(release?.createRelease?.id).toBeTruthy();
		expect(release?.createRelease?.createdAt).toBeTruthy();
	});

	test("enables application", async () => {
		const result = await enableApplication(
			{ applicationName: "test-app-1", storeId: "store-1" },
			{ accessToken: "test-token-123" },
		);

		expect(result?.enableStoreApplication?.id).toBe("app-1");
	});

	test("disables application", async () => {
		const result = await disableApplication(
			{ applicationName: "test-app-1", storeId: "store-1" },
			{ accessToken: "test-token-123" },
		);

		expect(result?.disableStoreApplication?.id).toBe("app-1");
	});

	test("archives application", async () => {
		const result = await archiveApplication("app-1", {
			accessToken: "test-token-123",
		});

		expect(result?.archiveApplication?.id).toBe("app-1");
		expect(result?.archiveApplication?.status).toBe("ARCHIVED");
		expect(result?.archiveApplication?.archivedAt).toBeTruthy();
	});
});
