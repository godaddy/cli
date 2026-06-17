import crypto from "node:crypto";
import type { FileSystem } from "@effect/platform/FileSystem";
import * as Effect from "effect/Effect";
import { ConfigurationError } from "../effect/errors";
import { Keychain, type KeychainCredential } from "../effect/services/keychain";
import {
  DEFAULT_ENVIRONMENT,
  type Environment,
  envGetEffect,
  getApiUrl,
  getClientId,
} from "./environment";

const KEYCHAIN_SERVICE = "godaddy-cli";
const LEGACY_TOKEN_KEY = "token";
const TOKEN_KEY_VERSION = "v3";
const LEGACY_SCOPED_TOKEN_KEY_VERSION = "v2";
const SCOPED_TOKEN_KEY_BYTES = 16;

interface StoredTokenPayload {
  accessToken: string;
  expiresAt: string;
}

export interface StoredToken {
  accessToken: string;
  expiresAt: Date;
}

function getEnvironmentTokenKey(environment: Environment): string {
  return `token:${environment}`;
}

function getScopedTokenKey(
  environment: Environment,
  tokenEndpoint: string,
  clientId: string,
): string {
  const scopeMaterial = `${environment}|${tokenEndpoint}`;
  const scopeHash = crypto
    .scryptSync(clientId, scopeMaterial, SCOPED_TOKEN_KEY_BYTES)
    .toString("hex");
  return `token:${TOKEN_KEY_VERSION}:${environment}:${scopeHash}`;
}

function getLegacyScopedTokenKeyPrefix(environment: Environment): string {
  return `token:${LEGACY_SCOPED_TOKEN_KEY_VERSION}:${environment}:`;
}

function getCurrentEnvironmentEffect(): Effect.Effect<
  Environment,
  never,
  FileSystem
> {
  return envGetEffect().pipe(Effect.orElseSucceed(() => DEFAULT_ENVIRONMENT));
}

function getTokenEndpoint(environment: Environment): string {
  if (process.env.OAUTH_TOKEN_URL) {
    return process.env.OAUTH_TOKEN_URL;
  }

  return `${getApiUrl(environment)}/v2/oauth2/token`;
}

function getOauthClientId(environment: Environment): string {
  return getClientId(environment);
}

function getKeyContext(environment: Environment): {
  scopedTokenKey: string;
  legacyEnvironmentTokenKey: string;
} {
  const tokenEndpoint = getTokenEndpoint(environment);
  const clientId = getOauthClientId(environment);
  return {
    scopedTokenKey: getScopedTokenKey(environment, tokenEndpoint, clientId),
    legacyEnvironmentTokenKey: getEnvironmentTokenKey(environment),
  };
}

function serializeToken(token: StoredToken): string {
  return JSON.stringify({
    accessToken: token.accessToken,
    expiresAt: token.expiresAt.toISOString(),
  } satisfies StoredTokenPayload);
}

function parseTokenValueEffect(
  value: string,
  tokenKey: string,
): Effect.Effect<StoredToken | null, never, Keychain> {
  return Effect.gen(function* () {
    const keychain = yield* Keychain;

    try {
      const parsed = JSON.parse(value) as Partial<StoredTokenPayload>;
      const accessToken = parsed.accessToken;
      const expiresAtValue = parsed.expiresAt;

      if (
        typeof accessToken !== "string" ||
        typeof expiresAtValue !== "string"
      ) {
        yield* Effect.promise(() =>
          keychain.deletePassword(KEYCHAIN_SERVICE, tokenKey),
        );
        return null;
      }

      const expiresAt = new Date(expiresAtValue);
      if (Number.isNaN(expiresAt.getTime())) {
        yield* Effect.promise(() =>
          keychain.deletePassword(KEYCHAIN_SERVICE, tokenKey),
        );
        return null;
      }

      if (expiresAt.getTime() <= Date.now()) {
        yield* Effect.promise(() =>
          keychain.deletePassword(KEYCHAIN_SERVICE, tokenKey),
        );
        return null;
      }

      return { accessToken, expiresAt };
    } catch {
      yield* Effect.promise(() =>
        keychain.deletePassword(KEYCHAIN_SERVICE, tokenKey),
      );
      return null;
    }
  });
}

function findLegacyScopedTokenEffect(
  environment: Environment,
): Effect.Effect<
  { tokenKey: string; token: StoredToken } | null,
  never,
  Keychain
> {
  return Effect.gen(function* () {
    const keychain = yield* Keychain;
    try {
      const legacyPrefix = getLegacyScopedTokenKeyPrefix(environment);
      const credentials = (yield* Effect.promise(() =>
        keychain.findCredentials(KEYCHAIN_SERVICE),
      )) as KeychainCredential[];

      for (const credential of credentials) {
        if (!credential.account.startsWith(legacyPrefix)) {
          continue;
        }

        const token = yield* parseTokenValueEffect(
          credential.password,
          credential.account,
        );
        if (token) {
          return { tokenKey: credential.account, token };
        }
      }
    } catch {
      // Ignore lookup failures and continue with other fallback keys.
    }

    return null;
  });
}

function deleteLegacyScopedTokensEffect(
  environment: Environment,
): Effect.Effect<void, never, Keychain> {
  return Effect.gen(function* () {
    const keychain = yield* Keychain;
    try {
      const legacyPrefix = getLegacyScopedTokenKeyPrefix(environment);
      const credentials = (yield* Effect.promise(() =>
        keychain.findCredentials(KEYCHAIN_SERVICE),
      )) as KeychainCredential[];
      const deletions = credentials
        .filter((credential: KeychainCredential) =>
          credential.account.startsWith(legacyPrefix),
        )
        .map((credential) =>
          keychain.deletePassword(KEYCHAIN_SERVICE, credential.account),
        );

      yield* Effect.promise(() => Promise.all(deletions));
    } catch {
      // Ignore cleanup failures.
    }
  });
}

export function saveTokenEffect(
  accessToken: string,
  expiresAt: Date,
  environment?: Environment,
): Effect.Effect<void, ConfigurationError, FileSystem | Keychain> {
  return Effect.gen(function* () {
    const keychain = yield* Keychain;
    const env = environment ?? (yield* getCurrentEnvironmentEffect());
    const { scopedTokenKey } = getKeyContext(env);
    const token = serializeToken({ accessToken, expiresAt });
    yield* Effect.tryPromise({
      try: () => keychain.setPassword(KEYCHAIN_SERVICE, scopedTokenKey, token),
      catch: (e) =>
        new ConfigurationError({
          message: `Failed to save token: ${e}`,
          userMessage: "Could not save authentication token to keychain",
        }),
    });
  });
}

export function getStoredTokenEffect(
  environment?: Environment,
): Effect.Effect<
  StoredToken | null,
  ConfigurationError,
  FileSystem | Keychain
> {
  return Effect.gen(function* () {
    const keychain = yield* Keychain;
    const env = environment ?? (yield* getCurrentEnvironmentEffect());
    const { scopedTokenKey, legacyEnvironmentTokenKey } = getKeyContext(env);

    const scopedValue = yield* Effect.tryPromise({
      try: () => keychain.getPassword(KEYCHAIN_SERVICE, scopedTokenKey),
      catch: (e) =>
        new ConfigurationError({
          message: `Failed to read token from keychain: ${e}`,
          userMessage:
            "Unable to access secure credentials. Unlock your keychain and try again.",
        }),
    });
    if (scopedValue) {
      return yield* parseTokenValueEffect(scopedValue, scopedTokenKey);
    }

    // Backward compatibility: migrate from previous environment-scoped key.
    const legacyEnvironmentValue = yield* Effect.tryPromise({
      try: () =>
        keychain.getPassword(KEYCHAIN_SERVICE, legacyEnvironmentTokenKey),
      catch: (e) =>
        new ConfigurationError({
          message: `Failed to read token from keychain: ${e}`,
          userMessage:
            "Unable to access secure credentials. Unlock your keychain and try again.",
        }),
    });
    if (legacyEnvironmentValue) {
      const legacyEnvironmentToken = yield* parseTokenValueEffect(
        legacyEnvironmentValue,
        legacyEnvironmentTokenKey,
      );
      if (legacyEnvironmentToken) {
        try {
          yield* Effect.promise(() =>
            keychain.setPassword(
              KEYCHAIN_SERVICE,
              scopedTokenKey,
              serializeToken(legacyEnvironmentToken),
            ),
          );
          yield* Effect.promise(() =>
            keychain.deletePassword(
              KEYCHAIN_SERVICE,
              legacyEnvironmentTokenKey,
            ),
          );
        } catch {
          // Non-fatal: return token even if migration write fails.
        }
        return legacyEnvironmentToken;
      }
    }

    // Backward compatibility: migrate from previous v2 scoped token key.
    const legacyScopedToken = yield* findLegacyScopedTokenEffect(env);
    if (legacyScopedToken) {
      try {
        yield* Effect.promise(() =>
          keychain.setPassword(
            KEYCHAIN_SERVICE,
            scopedTokenKey,
            serializeToken(legacyScopedToken.token),
          ),
        );
        yield* Effect.promise(() =>
          keychain.deletePassword(KEYCHAIN_SERVICE, legacyScopedToken.tokenKey),
        );
      } catch {
        // Non-fatal: return token even if migration write fails.
      }

      return legacyScopedToken.token;
    }

    // Backward compatibility: migrate from legacy token key if present.
    const legacyValue = yield* Effect.promise(() =>
      keychain.getPassword(KEYCHAIN_SERVICE, LEGACY_TOKEN_KEY),
    );
    if (!legacyValue) {
      return null;
    }

    const legacyToken = yield* parseTokenValueEffect(
      legacyValue,
      LEGACY_TOKEN_KEY,
    );
    if (!legacyToken) {
      return null;
    }

    try {
      yield* Effect.promise(() =>
        keychain.setPassword(
          KEYCHAIN_SERVICE,
          scopedTokenKey,
          serializeToken(legacyToken),
        ),
      );
      yield* Effect.promise(() =>
        keychain.deletePassword(KEYCHAIN_SERVICE, legacyEnvironmentTokenKey),
      );
      yield* Effect.promise(() =>
        keychain.deletePassword(KEYCHAIN_SERVICE, LEGACY_TOKEN_KEY),
      );
    } catch {
      // Non-fatal: return token even if migration write fails.
    }

    return legacyToken;
  });
}

export function deleteStoredTokenEffect(
  environment?: Environment,
): Effect.Effect<void, ConfigurationError, FileSystem | Keychain> {
  return Effect.gen(function* () {
    const keychain = yield* Keychain;
    const env = environment ?? (yield* getCurrentEnvironmentEffect());
    const { scopedTokenKey, legacyEnvironmentTokenKey } = getKeyContext(env);
    yield* Effect.promise(() =>
      keychain.deletePassword(KEYCHAIN_SERVICE, scopedTokenKey),
    );
    yield* deleteLegacyScopedTokensEffect(env);
    yield* Effect.promise(() =>
      keychain.deletePassword(KEYCHAIN_SERVICE, legacyEnvironmentTokenKey),
    );
    yield* Effect.promise(() =>
      keychain.deletePassword(KEYCHAIN_SERVICE, LEGACY_TOKEN_KEY),
    );
  });
}
