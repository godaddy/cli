import crypto from "node:crypto";
import http from "node:http";
import { URL } from "node:url";
import type { FileSystem } from "@effect/platform/FileSystem";
import * as Effect from "effect/Effect";
import { AuthenticationError, ConfigurationError } from "../effect/errors";
import { Browser } from "../effect/services/browser";

import type { Keychain } from "../effect/services/keychain";
// loggedFetch calls globalThis.fetch directly — not through the Fetch service tag.
// This is acceptable here because the OAuth callback runs inside a raw http.Server
// handler (Promise context), outside the Effect runtime. The Fetch tag is for
// code within Effect.gen contexts.
import { loggedFetch } from "../services/logger";
import { cliTraceHeaders } from "../shared/cli-trace";
import {
  type Environment,
  envGetEffect,
  getApiUrl,
  getClientId,
} from "./environment";
import {
  deleteStoredTokenEffect,
  getStoredTokenEffect,
  saveTokenEffect,
} from "./token-store";

const PORT = 7443;
const AUTH_HOST = "127.0.0.1";
const AUTH_TIMEOUT_MS = 120_000;
const DEFAULT_OAUTH_SCOPES = "apps.app-registry:read apps.app-registry:write";

/**
 * Escape a string for safe embedding in HTML text content.
 */
function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

export interface AuthResult {
  success: boolean;
  accessToken?: string;
  expiresAt?: Date;
  onboardingPending?: boolean;
}

export interface AuthStatus {
  authenticated: boolean;
  hasToken: boolean;
  tokenExpiry?: Date;
  environment: string;
}

export interface TokenInfo {
  accessToken: string;
  expiresAt: Date;
  expiresInSeconds: number;
}

let server: http.Server | null = null;

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

// Internal helper effects for OAuth configuration

function getOauthAuthUrlEffect(): Effect.Effect<string, never, FileSystem> {
  return Effect.gen(function* () {
    if (process.env.OAUTH_AUTH_URL) {
      return process.env.OAUTH_AUTH_URL;
    }
    const env = yield* envGetEffect().pipe(
      Effect.orElseSucceed(() => "ote" as Environment),
    );
    return `${getApiUrl(env)}/v2/oauth2/authorize`;
  });
}

function getOauthTokenUrlEffect(): Effect.Effect<string, never, FileSystem> {
  return Effect.gen(function* () {
    if (process.env.OAUTH_TOKEN_URL) {
      return process.env.OAUTH_TOKEN_URL;
    }
    const env = yield* envGetEffect().pipe(
      Effect.orElseSucceed(() => "ote" as Environment),
    );
    return `${getApiUrl(env)}/v2/oauth2/token`;
  });
}

function getOauthClientIdEffect(): Effect.Effect<string, never, FileSystem> {
  return Effect.gen(function* () {
    if (process.env.GODADDY_OAUTH_CLIENT_ID) {
      return process.env.GODADDY_OAUTH_CLIENT_ID;
    }
    const env = yield* envGetEffect().pipe(
      Effect.orElseSucceed(() => "ote" as Environment),
    );
    return getClientId(env);
  });
}

/**
 * Authenticate with GoDaddy OAuth
 */
export function authLoginEffect(options?: {
  additionalScopes?: string[];
}): Effect.Effect<
  AuthResult,
  AuthenticationError,
  FileSystem | Keychain | Browser
> {
  return Effect.gen(function* () {
    const browser = yield* Browser;

    const state = crypto.randomUUID();
    const codeVerifier = crypto.randomBytes(32).toString("base64url");
    const codeChallenge = crypto
      .createHash("sha256")
      .update(codeVerifier)
      .digest("base64url");

    const oauthAuthUrl = yield* getOauthAuthUrlEffect();
    const oauthTokenUrl = yield* getOauthTokenUrlEffect();
    const clientId = yield* getOauthClientIdEffect();

    const result = yield* Effect.tryPromise({
      try: () =>
        new Promise<AuthResult>((resolve, reject) => {
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

                const actualPort = (
                  server?.address() as import("net").AddressInfo
                )?.port;
                if (!actualPort) {
                  throw new Error(
                    "Could not determine server port for token exchange",
                  );
                }

                const tokenResponse = await loggedFetch(
                  oauthTokenUrl,
                  {
                    method: "POST",
                    headers: {
                      "Content-Type": "application/x-www-form-urlencoded",
                      ...cliTraceHeaders(),
                    },
                    body: new URLSearchParams({
                      client_id: clientId,
                      code,
                      grant_type: "authorization_code",
                      redirect_uri: `http://localhost:${actualPort}/callback`,
                      code_verifier: codeVerifier,
                    }),
                  },
                  {
                    includeRequestBody: false,
                    includeResponseBody: false,
                  },
                );

                if (!tokenResponse.ok) {
                  throw new Error(
                    `Token request failed: ${tokenResponse.status}`,
                  );
                }

                const tokenData = await tokenResponse.json();
                const expiresAt = new Date(
                  Date.now() + tokenData.expires_in * 1000,
                );

                res.writeHead(200, {
                  "Content-Type": "text/html",
                });
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
                  err instanceof Error
                    ? err.message
                    : "An unknown error occurred";
                console.error("Authentication callback error:", errorMessage);
                res.writeHead(500, {
                  "Content-Type": "text/html",
                });
                res.end(
                  `<html><body><h1>Authentication Failed</h1><p>${escapeHtml(errorMessage)}</p></body></html>`,
                );
                reject(err);
              } finally {
                clearTimeout(authTimeout);
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

          // Auto-close after timeout if the user never completes the flow
          const authTimeout = setTimeout(() => {
            if (server) {
              server.close();
              server = null;
            }
            reject(
              new Error(
                "Login timed out. The authentication flow was not completed.",
              ),
            );
          }, AUTH_TIMEOUT_MS);

          server.listen(PORT, AUTH_HOST, () => {
            const actualPort = (server?.address() as import("net").AddressInfo)
              ?.port;
            if (!actualPort) {
              const err = new Error(
                "Server started but could not determine port.",
              );
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
            const extra =
              options?.additionalScopes?.filter((s) => s.length > 0) ?? [];
            const scope =
              extra.length > 0
                ? `${DEFAULT_OAUTH_SCOPES} ${extra.join(" ")}`
                : DEFAULT_OAUTH_SCOPES;
            authUrl.searchParams.set("scope", scope);
            authUrl.searchParams.set("code_challenge", codeChallenge);
            authUrl.searchParams.set("code_challenge_method", "S256");

            browser.open(authUrl.toString());
          });
        }),
      catch: (error) =>
        new AuthenticationError({
          message: `Authentication failed: ${error}`,
          userMessage: "Authentication with GoDaddy failed. Please try again.",
        }),
    });

    // Save the token after the auth server flow completes
    if (result.accessToken && result.expiresAt) {
      yield* saveTokenEffect(result.accessToken, result.expiresAt).pipe(
        Effect.catchAll(() => Effect.void),
      );
    }

    return result;
  });
}

/**
 * Logout and clear stored credentials
 */
export function authLogoutEffect(): Effect.Effect<
  void,
  ConfigurationError,
  FileSystem | Keychain
> {
  return deleteStoredTokenEffect().pipe(
    Effect.catchAll((error) =>
      Effect.fail(
        new ConfigurationError({
          message: `Logout failed: ${error.message}`,
          userMessage: "Failed to clear stored credentials",
        }),
      ),
    ),
  );
}

/**
 * Get authentication status
 */
export function authStatusEffect(): Effect.Effect<
  AuthStatus,
  ConfigurationError,
  FileSystem | Keychain
> {
  return Effect.gen(function* () {
    const environment = yield* envGetEffect().pipe(
      Effect.orElseSucceed(() => "ote" as Environment),
    );
    const tokenInfo = yield* getTokenInfoEffect().pipe(
      Effect.catchAll(() => Effect.succeed(null)),
    );

    if (!tokenInfo) {
      return {
        authenticated: false,
        hasToken: false,
        environment,
      } satisfies AuthStatus;
    }

    return {
      authenticated: true,
      hasToken: true,
      tokenExpiry: tokenInfo.expiresAt,
      environment,
    } satisfies AuthStatus;
  });
}

/**
 * Get access token, authenticating if necessary
 */
export function getAccessTokenEffect(): Effect.Effect<
  string | null,
  AuthenticationError,
  FileSystem | Keychain
> {
  return getFromKeychainEffect("token").pipe(
    Effect.map((token) => token ?? null),
    Effect.catchAll(() =>
      Effect.fail(
        new AuthenticationError({
          message: "Failed to get access token",
          userMessage: "Could not retrieve access token",
        }),
      ),
    ),
  );
}

/**
 * Get token info including expiry details
 * Returns null if no token or token is expired
 */
export function getTokenInfoEffect(): Effect.Effect<
  TokenInfo | null,
  ConfigurationError,
  FileSystem | Keychain
> {
  return Effect.gen(function* () {
    const storedToken = yield* getStoredTokenEffect();
    if (!storedToken) return null;

    const expiresInSeconds = Math.floor(
      (storedToken.expiresAt.getTime() - Date.now()) / 1000,
    );

    return {
      accessToken: storedToken.accessToken,
      expiresAt: storedToken.expiresAt,
      expiresInSeconds,
    };
  });
}

/**
 * Get a value from the keychain by key
 */
export function getFromKeychainEffect(
  key: string,
): Effect.Effect<string | null, never, FileSystem | Keychain> {
  return Effect.gen(function* () {
    if (key !== "token") {
      return null;
    }

    const storedToken = yield* getStoredTokenEffect().pipe(
      Effect.catchAll(() => Effect.succeed(null)),
    );
    return storedToken?.accessToken ?? null;
  });
}
