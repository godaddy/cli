import crypto from "node:crypto";
import http from "node:http";
import { URL } from "node:url";
import keytar from "keytar"; // This will now be handled by our build plugin
import openBrowser from "open";
import {
	type Environment,
	envGet,
	getApiUrl,
	getClientId,
} from "../core/environment";

const KEYCHAIN_SERVICE = "godaddy-cli";

const PORT = 7443;
const OAUTH_SCOPE = "apps.app-registry:read apps.app-registry:write";

let server: http.Server | null = null;

async function getEnvironment(): Promise<Environment> {
	const result = await envGet();
	if (!result.success || !result.data) {
		throw result.error ?? new Error("Failed to get environment");
	}
	return result.data as Environment;
}

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

export async function authenticate() {
	const state = crypto.randomUUID();
	const codeVerifier = crypto.randomBytes(32).toString("base64url");
	const codeChallenge = crypto
		.createHash("sha256")
		.update(codeVerifier)
		.digest("base64url");

	const oauthAuthUrl = await getOauthAuthUrl();
	const oauthTokenUrl = await getOauthTokenUrl();
	const clientId = await getOauthClientId();

	return new Promise((resolve, reject) => {
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
					const expiresAt = new Date(Date.now() + tokenData.expires_in * 1000);
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
					resolve({ success: true });
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
					if (server) server.close(); // Close server after handling callback
				}
			} else {
				// Handle other requests (e.g., favicon.ico) or methods
				res.writeHead(404);
				res.end();
			}
		});

		server.on("error", (err) => {
			console.error("Server startup error:", err);
			reject(err); // Reject promise if server fails to start
		});

		server.listen(PORT, () => {
			const actualPort = (server?.address() as import("net").AddressInfo)?.port;
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
}

export function stopAuthServer() {
	if (server) {
		server.close(() => {
			// Optional: console log or perform action on successful close
		});
		server = null;
	}
}

export async function logout(): Promise<void> {
	await keytar.deletePassword(KEYCHAIN_SERVICE, "token");
}

export async function getAccessToken() {
	const existingToken = await getFromKeychain("token");
	if (existingToken) {
		return existingToken;
	}

	await authenticate();
	const newToken = await getFromKeychain("token");
	return newToken;
}
