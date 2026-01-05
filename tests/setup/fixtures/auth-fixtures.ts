export const authFixtures = {
	validTokenResponse: {
		access_token: "test-token-123",
		token_type: "Bearer",
		expires_in: 3600,
		refresh_token: "test-refresh-token",
		scope: "apps.app-registry:read apps.app-registry:write",
	},

	expiredTokenError: {
		error: "invalid_grant",
		error_description: "Token has expired",
	},

	invalidCodeError: {
		error: "invalid_grant",
		error_description: "Invalid authorization code",
	},

	unauthorizedError: {
		errors: [
			{
				message: "Unauthorized",
				extensions: {
					code: "UNAUTHENTICATED",
				},
			},
		],
	},

	tokenExpiredError: {
		errors: [
			{
				message: "Token expired",
				extensions: {
					code: "TOKEN_EXPIRED",
				},
			},
		],
	},
};
