import { http, HttpResponse } from "msw";
import { authFixtures } from "../fixtures/auth-fixtures";

export const authHandlers = [
	// OAuth token exchange endpoint
	http.post("*/v2/oauth2/token", async ({ request }) => {
		const body = await request.formData();
		const grantType = body.get("grant_type");
		const code = body.get("code");
		const clientId = body.get("client_id");
		const codeVerifier = body.get("code_verifier");

		// Validate required parameters
		if (!grantType || !clientId) {
			return HttpResponse.json(
				{
					error: "invalid_request",
					error_description: "Missing required parameters",
				},
				{ status: 400 },
			);
		}

		// Handle authorization code flow
		if (grantType === "authorization_code") {
			if (code === "test-auth-code" && codeVerifier) {
				return HttpResponse.json(authFixtures.validTokenResponse);
			}

			if (code === "expired-auth-code") {
				return HttpResponse.json(authFixtures.expiredTokenError, {
					status: 400,
				});
			}

			return HttpResponse.json(authFixtures.invalidCodeError, { status: 400 });
		}

		// Handle refresh token flow
		if (grantType === "refresh_token") {
			const refreshToken = body.get("refresh_token");

			if (refreshToken === "test-refresh-token") {
				return HttpResponse.json({
					...authFixtures.validTokenResponse,
					access_token: "refreshed-token-456",
				});
			}

			return HttpResponse.json(
				{ error: "invalid_grant", error_description: "Invalid refresh token" },
				{ status: 400 },
			);
		}

		return HttpResponse.json(
			{
				error: "unsupported_grant_type",
				error_description: "Grant type not supported",
			},
			{ status: 400 },
		);
	}),

	// OAuth authorization endpoint (for URL validation and redirect handling)
	http.get("*/v2/oauth2/authorize", ({ request }) => {
		const url = new URL(request.url);
		const clientId = url.searchParams.get("client_id");
		const responseType = url.searchParams.get("response_type");
		const redirectUri = url.searchParams.get("redirect_uri");
		const state = url.searchParams.get("state");
		const codeChallenge = url.searchParams.get("code_challenge");
		const codeChallengeMethod = url.searchParams.get("code_challenge_method");

		// Validate required OAuth parameters
		if (!clientId) {
			return HttpResponse.text("Missing client_id parameter", { status: 400 });
		}

		if (responseType !== "code") {
			return HttpResponse.text("Invalid response_type", { status: 400 });
		}

		if (!redirectUri) {
			return HttpResponse.text("Missing redirect_uri parameter", {
				status: 400,
			});
		}

		if (!state) {
			return HttpResponse.text("Missing state parameter", { status: 400 });
		}

		if (!codeChallenge || codeChallengeMethod !== "S256") {
			return HttpResponse.text("Invalid PKCE parameters", { status: 400 });
		}

		// For testing, return HTML that simulates authorization page
		const authPageHtml = `
			<html>
				<head><title>OAuth Authorization (Test)</title></head>
				<body>
					<h1>Authorize Application</h1>
					<p>Test authorization page for client: ${clientId}</p>
					<script>
						// Simulate successful authorization after 1 second
						setTimeout(() => {
							window.location.href = '${redirectUri}?code=test-auth-code&state=${state}';
						}, 1000);
					</script>
				</body>
			</html>
		`;

		return HttpResponse.html(authPageHtml);
	}),

	// OAuth callback simulation (for testing redirect handling)
	http.get("*/callback", ({ request }) => {
		const url = new URL(request.url);
		const code = url.searchParams.get("code");
		const state = url.searchParams.get("state");
		const error = url.searchParams.get("error");

		if (error) {
			return HttpResponse.html(
				`
				<html>
					<body>
						<h1>Authorization Failed</h1>
						<p>Error: ${error}</p>
					</body>
				</html>
			`,
				{ status: 400 },
			);
		}

		if (code && state) {
			return HttpResponse.html(`
				<html>
					<body>
						<h1>Authorization Successful!</h1>
						<p>You can close this window now.</p>
					</body>
				</html>
			`);
		}

		return HttpResponse.html(
			`
			<html>
				<body>
					<h1>Invalid Request</h1>
					<p>Missing required parameters</p>
				</body>
			</html>
		`,
			{ status: 400 },
		);
	}),
];
