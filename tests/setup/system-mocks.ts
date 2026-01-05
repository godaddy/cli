import { vi } from "vitest";

// Mock keytar for secure storage
export const mockKeytar = {
	setPassword: vi.fn().mockResolvedValue(undefined),
	getPassword: vi.fn().mockResolvedValue(null),
	deletePassword: vi.fn().mockResolvedValue(true),
};

// Mock open for browser launching
export const mockOpen = vi.fn().mockResolvedValue(undefined);

// Mock environment variables
export const mockEnv = {
	OAUTH_AUTH_URL: "https://test-api.godaddy.com/v2/oauth2/authorize",
	OAUTH_TOKEN_URL: "https://test-api.godaddy.com/v2/oauth2/token",
	OAUTH_CLIENT_ID: "test-client-id",
	APPLICATIONS_GRAPHQL_URL:
		"https://test-api.godaddy.com/v1/apps/app-registry-subgraph",
};

// Global mock setup
vi.mock("keytar", () => ({ default: mockKeytar }));
vi.mock("open", () => ({ default: mockOpen }));

// Environment setup helpers
export function setupTestEnvironment() {
	Object.assign(process.env, mockEnv);
}

export function resetTestEnvironment() {
	for (const key of Object.keys(mockEnv)) {
		delete process.env[key];
	}
}

// Token helpers for tests
export const mockValidToken = () => {
	mockKeytar.getPassword.mockResolvedValue(
		JSON.stringify({
			accessToken: "test-token-123",
			expiresAt: new Date(Date.now() + 3600000).toISOString(),
		}),
	);
};

export const mockExpiredToken = () => {
	mockKeytar.getPassword.mockResolvedValue(
		JSON.stringify({
			accessToken: "expired-token",
			expiresAt: new Date(Date.now() - 1000).toISOString(),
		}),
	);
};

export const mockNoToken = () => {
	mockKeytar.getPassword.mockResolvedValue(null);
};
