import { Command } from "commander";
import {
	type AuthResult,
	type AuthStatus,
	authLogin,
	authLogout,
	authStatus,
} from "../../core/auth";

export function createAuthCommand(): Command {
	const auth = new Command("auth").description(
		"Manage authentication with GoDaddy Developer Platform",
	);

	// auth login (explicit)
	auth
		.command("login")
		.description("Login to GoDaddy Developer Platform")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (options) => {
			const result = await authLogin();

			if (!result.success) {
				console.error(result.error?.userMessage || "Authentication failed");
				process.exit(1);
			}

			const authResult = result.data as AuthResult;

			if (options.output === "json") {
				console.log(
					JSON.stringify(
						{
							success: authResult.success,
							authenticated: true,
							expiresAt: authResult.expiresAt?.toISOString(),
						},
						null,
						2,
					),
				);
			} else {
				console.log(
					"✅ Successfully authenticated with GoDaddy Developer Platform!",
				);
				if (authResult.expiresAt) {
					console.log(
						`Token expires: ${authResult.expiresAt.toLocaleString()}`,
					);
				}
			}
			process.exit(0);
		});

	// auth logout
	auth
		.command("logout")
		.description("Logout and clear stored credentials")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (options) => {
			const result = await authLogout();

			if (!result.success) {
				console.error(result.error?.userMessage || "Logout failed");
				process.exit(1);
			}

			if (options.output === "json") {
				console.log(
					JSON.stringify({ success: true, authenticated: false }, null, 2),
				);
			} else {
				console.log("✅ Successfully logged out. Credentials cleared.");
			}
			process.exit(0);
		});

	// auth status
	auth
		.command("status")
		.description("Check authentication status")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (options) => {
			const result = await authStatus();

			if (!result.success) {
				console.error(
					result.error?.userMessage || "Failed to check authentication status",
				);
				process.exit(1);
			}

			const status = result.data as AuthStatus;

			if (options.output === "json") {
				console.log(
					JSON.stringify(
						{
							authenticated: status.authenticated,
							hasToken: status.hasToken,
							environment: status.environment,
							tokenExpiry: status.tokenExpiry?.toISOString(),
						},
						null,
						2,
					),
				);
			} else {
				if (status.authenticated) {
					console.log(
						"✅ You are authenticated with GoDaddy Developer Platform",
					);
					console.log(`Environment: ${status.environment.toUpperCase()}`);
					if (status.tokenExpiry) {
						console.log(
							`Token expires: ${status.tokenExpiry.toLocaleString()}`,
						);
					}
				} else {
					console.log("❌ You are not authenticated");
					console.log(`Environment: ${status.environment.toUpperCase()}`);
					console.log("Run 'godaddy auth' to authenticate");
				}
			}
			process.exit(0);
		});

	return auth;
}
