import { afterAll, afterEach, beforeAll, vi } from "vitest";
import { server } from "./msw-server";
import { resetTestEnvironment, setupTestEnvironment } from "./system-mocks";

// MSW setup
beforeAll(() => {
	server.listen({
		onUnhandledRequest: "error", // Fail tests on unmocked requests
	});
	setupTestEnvironment();
});

afterEach(() => {
	server.resetHandlers();
	vi.clearAllMocks();
});

afterAll(() => {
	server.close();
	resetTestEnvironment();
});
