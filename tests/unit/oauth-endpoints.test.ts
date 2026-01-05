import { describe, expect, test } from "vitest";

describe("OAuth Endpoints", () => {
	test("token endpoint returns valid token for correct code", async () => {
		const response = await fetch(
			"https://test-api.godaddy.com/v2/oauth2/token",
			{
				method: "POST",
				headers: {
					"Content-Type": "application/x-www-form-urlencoded",
				},
				body: new URLSearchParams({
					grant_type: "authorization_code",
					client_id: "test-client-id",
					code: "test-auth-code",
					redirect_uri: "http://localhost:7443/callback",
					code_verifier: "test-verifier",
				}),
			},
		);

		expect(response.status).toBe(200);
		const data = await response.json();
		expect(data.access_token).toBe("test-token-123");
		expect(data.token_type).toBe("Bearer");
	});

	test("token endpoint returns error for invalid code", async () => {
		const response = await fetch(
			"https://test-api.godaddy.com/v2/oauth2/token",
			{
				method: "POST",
				headers: {
					"Content-Type": "application/x-www-form-urlencoded",
				},
				body: new URLSearchParams({
					grant_type: "authorization_code",
					client_id: "test-client-id",
					code: "invalid-code",
					redirect_uri: "http://localhost:7443/callback",
					code_verifier: "test-verifier",
				}),
			},
		);

		expect(response.status).toBe(400);
		const data = await response.json();
		expect(data.error).toBe("invalid_grant");
	});

	test("token endpoint handles refresh token flow", async () => {
		const response = await fetch(
			"https://test-api.godaddy.com/v2/oauth2/token",
			{
				method: "POST",
				headers: {
					"Content-Type": "application/x-www-form-urlencoded",
				},
				body: new URLSearchParams({
					grant_type: "refresh_token",
					client_id: "test-client-id",
					refresh_token: "test-refresh-token",
				}),
			},
		);

		expect(response.status).toBe(200);
		const data = await response.json();
		expect(data.access_token).toBe("refreshed-token-456");
		expect(data.token_type).toBe("Bearer");
	});

	test("token endpoint validates required parameters", async () => {
		const response = await fetch(
			"https://test-api.godaddy.com/v2/oauth2/token",
			{
				method: "POST",
				headers: {
					"Content-Type": "application/x-www-form-urlencoded",
				},
				body: new URLSearchParams({
					grant_type: "authorization_code",
					// Missing client_id
				}),
			},
		);

		expect(response.status).toBe(400);
		const data = await response.json();
		expect(data.error).toBe("invalid_request");
	});

	test("authorize endpoint validates parameters", async () => {
		const response = await fetch(
			`https://test-api.godaddy.com/v2/oauth2/authorize?${new URLSearchParams({
				client_id: "test-client-id",
				response_type: "code",
				redirect_uri: "http://localhost:7443/callback",
				state: "test-state",
				code_challenge: "test-challenge",
				code_challenge_method: "S256",
			})}`,
		);

		expect(response.status).toBe(200);
		const html = await response.text();
		expect(html).toContain("Authorize Application");
		expect(html).toContain("test-client-id");
	});

	test("authorize endpoint rejects missing parameters", async () => {
		const response = await fetch(
			`https://test-api.godaddy.com/v2/oauth2/authorize?${new URLSearchParams({
				client_id: "test-client-id",
				// Missing required parameters
			})}`,
		);

		expect(response.status).toBe(400);
		const text = await response.text();
		expect(text).toContain("Invalid response_type");
	});

	test("callback endpoint handles successful authorization", async () => {
		const response = await fetch(
			`http://localhost:7443/callback?${new URLSearchParams({
				code: "test-auth-code",
				state: "test-state",
			})}`,
		);

		expect(response.status).toBe(200);
		const html = await response.text();
		expect(html).toContain("Authorization Successful!");
	});

	test("callback endpoint handles authorization errors", async () => {
		const response = await fetch(
			`http://localhost:7443/callback?${new URLSearchParams({
				error: "access_denied",
				state: "test-state",
			})}`,
		);

		expect(response.status).toBe(400);
		const html = await response.text();
		expect(html).toContain("Authorization Failed");
		expect(html).toContain("access_denied");
	});
});
