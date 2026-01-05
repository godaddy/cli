import { HttpResponse } from "msw";
import { authFixtures } from "./fixtures/auth-fixtures";

// Check if request has valid authentication
export const checkAuthentication = (request: Request) => {
	const auth = request.headers.get("Authorization");

	if (!auth) {
		return HttpResponse.json(authFixtures.unauthorizedError, { status: 401 });
	}

	if (!auth.startsWith("Bearer ")) {
		return HttpResponse.json(authFixtures.unauthorizedError, { status: 401 });
	}

	const token = auth.replace("Bearer ", "");

	// Check for valid test tokens
	if (token === "test-token-123" || token === "refreshed-token-456") {
		return null; // Valid authentication
	}

	// Check for expired token
	if (token === "expired-token") {
		return HttpResponse.json(authFixtures.tokenExpiredError, { status: 401 });
	}

	// Invalid token
	return HttpResponse.json(authFixtures.unauthorizedError, { status: 401 });
};

// Extract token from Authorization header
export const extractToken = (request: Request): string | null => {
	const auth = request.headers.get("Authorization");

	if (!auth || !auth.startsWith("Bearer ")) {
		return null;
	}

	return auth.replace("Bearer ", "");
};

// Check if token is valid for specific scopes
export const hasScope = (token: string, requiredScope: string): boolean => {
	// In real implementation, this would decode JWT and check scopes
	// For testing, we'll use simple token-based logic
	const validTokens = ["test-token-123", "refreshed-token-456"];

	if (!validTokens.includes(token)) {
		return false;
	}

	// For testing, assume all valid tokens have required scopes
	const supportedScopes = ["apps.app-registry:read", "apps.app-registry:write"];

	return supportedScopes.includes(requiredScope);
};
