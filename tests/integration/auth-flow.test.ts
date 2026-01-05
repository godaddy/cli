import { beforeEach, describe, expect, test } from "vitest";
import { getFromKeychain } from "../../src/services/auth";
import { mockKeytar } from "../setup/system-mocks";
import { withNoAuth, withValidAuth } from "../setup/test-utils";

describe("Authentication Flow", () => {
	beforeEach(() => {
		withNoAuth();
	});

	test("getFromKeychain returns valid token when present", async () => {
		withValidAuth();

		const token = await getFromKeychain("token");
		expect(token).toBe("test-token-123");
	});

	test("getFromKeychain returns null for expired token", async () => {
		// Mock expired token
		mockKeytar.getPassword.mockResolvedValueOnce(
			JSON.stringify({
				accessToken: "expired-token",
				expiresAt: new Date(Date.now() - 1000).toISOString(),
			}),
		);
		mockKeytar.deletePassword.mockResolvedValueOnce(true);

		const token = await getFromKeychain("token");
		expect(token).toBeNull();

		// Should have deleted expired token
		expect(mockKeytar.deletePassword).toHaveBeenCalledWith(
			"godaddy-cli",
			"token",
		);
	});

	test("getFromKeychain returns null when no token exists", async () => {
		withNoAuth();

		const token = await getFromKeychain("token");
		expect(token).toBeNull();
	});

	test("token validation works correctly", async () => {
		// Test various token scenarios
		const scenarios = [
			{
				name: "valid token",
				token: {
					accessToken: "valid-token",
					expiresAt: new Date(Date.now() + 3600000).toISOString(),
				},
				expected: "valid-token",
			},
			{
				name: "expired token",
				token: {
					accessToken: "expired-token",
					expiresAt: new Date(Date.now() - 1000).toISOString(),
				},
				expected: null,
			},
		];

		for (const scenario of scenarios) {
			mockKeytar.getPassword.mockResolvedValueOnce(
				JSON.stringify(scenario.token),
			);
			if (scenario.expected === null) {
				mockKeytar.deletePassword.mockResolvedValueOnce(true);
			}

			const result = await getFromKeychain("token");
			expect(result).toBe(scenario.expected);
		}
	});
});
