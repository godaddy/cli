import { describe, expect, test } from "vitest";
import { getFromKeychain } from "../../src/services/auth";
import {
	withExpiredAuth,
	withNoAuth,
	withValidAuth,
} from "../setup/test-utils";

describe("Auth Service", () => {
	test("returns valid token when present", async () => {
		withValidAuth();

		const token = await getFromKeychain("token");
		expect(token).toBe("test-token-123");
	});

	test("returns null for expired token", async () => {
		withExpiredAuth();

		const token = await getFromKeychain("token");
		expect(token).toBeNull();
	});

	test("returns null when no token exists", async () => {
		withNoAuth();

		const token = await getFromKeychain("token");
		expect(token).toBeNull();
	});
});
