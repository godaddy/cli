import crypto from "node:crypto";
import http from "node:http";
import { URL } from "node:url";
import keytar from "keytar";
import openBrowser from "open";
import {
	AuthenticationError,
	type CmdResult,
	ConfigurationError,
	NetworkError,
} from "../shared/types";
import {
	type Environment,
	envGet,
	getApiUrl,
	getClientId,
} from "./environment";

const KEYCHAIN_SERVICE = "godaddy-cli";
const PORT = 7443;
const OAUTH_SCOPE = "apps.app-registry:read apps.app-registry:write";

export interface AuthResult {
	success: boolean;
	accessToken?: string;
	expiresAt?: Date;
}

export interface AuthStatus {
	authenticated: boolean;
	hasToken: boolean;
	tokenExpiry?: Date;
	environment: string;
}

let server: http.Server | null = null;

/**
 * Authenticate with GoDaddy OAuth
 */
export async function authLogin(): Promise<CmdResult<AuthResult>> {
	try {
		const state = crypto.randomUUID();
		const codeVerifier = crypto.randomBytes(32).toString("base64url");
		const codeChallenge = crypto
			.createHash("sha256")
			.update(codeVerifier)
			.digest("base64url");

		const oauthAuthUrl = await getOauthAuthUrl();
		const oauthTokenUrl = await getOauthTokenUrl();
		const clientId = await getOauthClientId();

		const result = await new Promise<AuthResult>((resolve, reject) => {
			server = http.createServer(async (req, res) => {
				if (!req.url || !req.headers.host) {
					res.writeHead(400);
					res.end("Bad Request");
					reject(new Error("Missing request URL or host"));
					if (server) server.close();
					return;
				}

				const requestUrl = new URL(req.url, `http://${req.headers.host}`);
				const params = requestUrl.searchParams;

				if (requestUrl.pathname === "/callback" && req.method === "GET") {
					const receivedState = params.get("state");
					const code = params.get("code");
					const error = params.get("error");

					try {
						if (receivedState !== state) {
							throw new Error("State mismatch");
						}

						if (error) {
							throw new Error(`Authentication error: ${error}`);
						}

						if (!code) {
							throw new Error("No code received");
						}

						const actualPort = (server?.address() as import("net").AddressInfo)
							?.port;
						if (!actualPort) {
							throw new Error(
								"Could not determine server port for token exchange",
							);
						}

						const tokenResponse = await fetch(oauthTokenUrl, {
							method: "POST",
							headers: {
								"Content-Type": "application/x-www-form-urlencoded",
							},
							body: new URLSearchParams({
								client_id: clientId,
								code,
								grant_type: "authorization_code",
								redirect_uri: `http://localhost:${actualPort}/callback`,
								code_verifier: codeVerifier,
							}),
						});

						if (!tokenResponse.ok) {
							throw new Error(`Token request failed: ${tokenResponse.status}`);
						}

						const tokenData = await tokenResponse.json();
						const expiresAt = new Date(
							Date.now() + tokenData.expires_in * 1000,
						);
						await saveToKeychain(
							"token",
							JSON.stringify({
								accessToken: tokenData.access_token,
								expiresAt,
							}),
						);

						res.writeHead(200, { "Content-Type": "text/html" });
						res.end(
							"<html><body><h1>Authentication successful!</h1><p>You can close this window now.</p></body></html>",
						);
						resolve({
							success: true,
							accessToken: tokenData.access_token,
							expiresAt,
						});
					} catch (err: unknown) {
						const errorMessage =
							err instanceof Error ? err.message : "An unknown error occurred";
						console.error("Authentication callback error:", errorMessage);
						res.writeHead(500, { "Content-Type": "text/html" });
						res.end(
							`<html><body><h1>Authentication Failed</h1><p>${errorMessage}</p></body></html>`,
						);
						reject(err);
					} finally {
						if (server) server.close();
					}
				} else {
					res.writeHead(404);
					res.end();
				}
			});

			server.on("error", (err) => {
				console.error("Server startup error:", err);
				reject(err);
			});

			server.listen(PORT, () => {
				const actualPort = (server?.address() as import("net").AddressInfo)
					?.port;
				if (!actualPort) {
					const err = new Error("Server started but could not determine port.");
					console.error(err);
					if (server) server.close();
					reject(err);
					return;
				}

				const authUrl = new URL(oauthAuthUrl);
				authUrl.searchParams.set("client_id", clientId);
				authUrl.searchParams.set("response_type", "code");
				authUrl.searchParams.set(
					"redirect_uri",
					`http://localhost:${actualPort}/callback`,
				);
				authUrl.searchParams.set("state", state);
				authUrl.searchParams.set("scope", OAUTH_SCOPE);
				authUrl.searchParams.set("code_challenge", codeChallenge);
				authUrl.searchParams.set("code_challenge_method", "S256");

				openBrowser(authUrl.toString());
			});
		});

		return { success: true, data: result };
	} catch (error) {
		return {
			success: false,
			error: new AuthenticationError(
				`Authentication failed: ${error}`,
				"Authentication with GoDaddy failed. Please try again.",
			),
		};
	}
}

/**
 * Logout and clear stored credentials
 */
export async function authLogout(): Promise<CmdResult<void>> {
	try {
		await keytar.deletePassword(KEYCHAIN_SERVICE, "token");
		return { success: true };
	} catch (error) {
		return {
			success: false,
			error: new ConfigurationError(
				`Logout failed: ${error}`,
				"Failed to clear stored credentials",
			),
		};
	}
}

async function getEnvironment(): Promise<Environment> {
	const result = await envGet();
	if (!result.success || !result.data) {
		throw result.error ?? new Error("Failed to get environment");
	}
	return result.data as Environment;
}

/**
 * Get authentication status
 */
export async function authStatus(): Promise<CmdResult<AuthStatus>> {
	try {
		const environment = await getEnvironment();
		const tokenData = await getFromKeychain("token");

		if (!tokenData) {
			return {
				success: true,
				data: {
					authenticated: false,
					hasToken: false,
					environment,
				},
			};
		}

		// If we have a token, parse it to check expiry
		try {
			const value = await keytar.getPassword(KEYCHAIN_SERVICE, "token");
			if (!value) {
				return {
					success: true,
					data: {
						authenticated: false,
						hasToken: false,
						environment,
					},
				};
			}

			const { expiresAt } = JSON.parse(value);
			const expiryDate = new Date(expiresAt);
			const isExpired = expiryDate.getTime() < Date.now();

			if (isExpired) {
				// Clean up expired token
				await keytar.deletePassword(KEYCHAIN_SERVICE, "token");
				return {
					success: true,
					data: {
						authenticated: false,
						hasToken: false,
						environment,
					},
				};
			}

			return {
				success: true,
				data: {
					authenticated: true,
					hasToken: true,
					tokenExpiry: expiryDate,
					environment,
				},
			};
		} catch (parseError) {
			// Invalid token format, clean it up
			await keytar.deletePassword(KEYCHAIN_SERVICE, "token");
			return {
				success: true,
				data: {
					authenticated: false,
					hasToken: false,
					environment,
				},
			};
		}
	} catch (error) {
		return {
			success: false,
			error: new ConfigurationError(
				`Failed to check authentication status: ${error}`,
				"Could not determine authentication status",
			),
		};
	}
}

/**
 * Get access token, authenticating if necessary (legacy compatibility)
 */
export async function getAccessToken(): Promise<string | null> {
	const existingToken = await getFromKeychain("token");
	if (existingToken) {
		return existingToken;
	}

	const result = await authLogin();
	if (!result.success) {
		return null;
	}

	const newToken = await getFromKeychain("token");
	return newToken;
}

/**
 * Stop the auth server (cleanup)
 */
export function stopAuthServer(): void {
	if (server) {
		server.close(() => {
			// Optional: console log or perform action on successful close
		});
		server = null;
	}
}

// Internal helper functions
async function getOauthAuthUrl(): Promise<string> {
	if (process.env.OAUTH_AUTH_URL) {
		return process.env.OAUTH_AUTH_URL;
	}
	const env = await getEnvironment();
	return `${getApiUrl(env)}/v2/oauth2/authorize`;
}

async function getOauthTokenUrl(): Promise<string> {
	if (process.env.OAUTH_TOKEN_URL) {
		return process.env.OAUTH_TOKEN_URL;
	}
	const env = await getEnvironment();
	return `${getApiUrl(env)}/v2/oauth2/token`;
}

async function getOauthClientId(): Promise<string> {
	if (process.env.GODADDY_OAUTH_CLIENT_ID) {
		return process.env.GODADDY_OAUTH_CLIENT_ID;
	}
	const env = await getEnvironment();
	return getClientId(env);
}

function saveToKeychain(key: string, value: string): Promise<void> {
	return keytar.setPassword(KEYCHAIN_SERVICE, key, value);
}

export async function getFromKeychain(key: string): Promise<string | null> {
	const value = await keytar.getPassword(KEYCHAIN_SERVICE, key);
	if (!value) return null;

	const { accessToken, expiresAt } = JSON.parse(value);
	if (new Date(expiresAt).getTime() < Date.now()) {
		await keytar.deletePassword(KEYCHAIN_SERVICE, key);
		return null;
	}

	return accessToken;
}

// Legacy compatibility function - use authLogin() instead
export async function authenticate(): Promise<{ success: boolean }> {
	const result = await authLogin();
	return { success: result.success };
}

// Legacy compatibility function - use authLogout() instead
export async function logout(): Promise<void> {
	await authLogout();
}
