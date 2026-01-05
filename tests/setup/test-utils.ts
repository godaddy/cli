import { expect } from "vitest";
import { mockExpiredToken, mockNoToken, mockValidToken } from "./system-mocks";

// Authentication state helpers
export const withValidAuth = <T>(
	callback?: (accessToken: string) => Promise<T>,
) => {
	mockValidToken();
	if (callback) {
		return callback("test-token-123");
	}
};

export const withExpiredAuth = <T>(
	callback?: (accessToken: string) => Promise<T>,
) => {
	mockExpiredToken();
	if (callback) {
		return callback("expired-token");
	}
};

export const withNoAuth = <T>(callback?: () => Promise<T>) => {
	mockNoToken();
	if (callback) {
		return callback();
	}
};

// Common test assertions
export const expectAuthRequired = (result: string) => {
	expect(
		result.includes("authentication required") ||
			result.includes("Please login first"),
	).toBe(true);
};

export const expectSuccess = (result: string) => {
	expect(result).not.toContain("error");
	expect(result).not.toContain("failed");
};
