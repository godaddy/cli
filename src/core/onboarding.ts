import type { FileSystem } from "@effect/platform/FileSystem";
import * as Effect from "effect/Effect";
import { AuthenticationError, ConfigurationError } from "../effect/errors";
import type { Keychain } from "../effect/services/keychain";
import { getTokenInfoEffect } from "./auth";
import { envGetEffect, getDevxCoreUrl } from "./environment";

export interface OnboardingStatus {
  orgId: string;
  status: string;
}

/**
 * Check onboarding status for the authenticated user via devx-core.
 * Auto-creates a PENDING org if none exists yet.
 */
export function checkOnboardingStatusEffect(): Effect.Effect<
  OnboardingStatus,
  ConfigurationError | AuthenticationError,
  FileSystem | Keychain
> {
  return Effect.gen(function* () {
    const tokenInfo = yield* getTokenInfoEffect().pipe(
      Effect.mapError(
        (err) =>
          new ConfigurationError({
            message: `Failed to get token: ${err.message}`,
            userMessage: "Could not check onboarding status.",
          }),
      ),
    );

    if (!tokenInfo) {
      return yield* Effect.fail(
        new AuthenticationError({
          message: "No token available for onboarding status check",
          userMessage: "Not authenticated.",
        }),
      );
    }

    const env = yield* envGetEffect().pipe(
      Effect.orElseSucceed(() => "ote" as const),
    );
    const baseUrl = getDevxCoreUrl(env);

    const response = yield* Effect.tryPromise({
      try: () =>
        fetch(`${baseUrl}/api/v1/onboarding/status`, {
          method: "POST",
          headers: {
            Authorization: `Bearer ${tokenInfo.accessToken}`,
            "Content-Type": "application/json",
          },
          body: JSON.stringify({}),
        }).then(async (res) => {
          if (res.status === 401) {
            throw new AuthenticationError({
              message: "Onboarding status check: unauthorized (401)",
              userMessage: "Session expired. Run 'godaddy auth login' again.",
            });
          }
          if (!res.ok) {
            const body = await res.text().catch(() => "");
            throw new ConfigurationError({
              message: `Onboarding status check failed: HTTP ${res.status} ${body}`,
              userMessage: "Could not check onboarding status.",
            });
          }
          return res.json();
        }),
      catch: (err) => {
        if (
          err instanceof AuthenticationError ||
          err instanceof ConfigurationError
        )
          return err;
        return new ConfigurationError({
          message: `Onboarding status check failed: ${err}`,
          userMessage: "Could not check onboarding status.",
        });
      },
    });

    const envelope = response as {
      success?: boolean;
      data?: { id?: string; status?: string };
    };
    const data =
      envelope.data ?? (response as { id?: string; status?: string });
    if (!data?.id || !data?.status) {
      return yield* Effect.fail(
        new ConfigurationError({
          message: "Unexpected onboarding status response shape",
          userMessage: "Could not check onboarding status.",
        }),
      );
    }

    return { orgId: data.id, status: data.status };
  });
}

/**
 * Complete CLI onboarding in one call — get/create org, accept all agreements, submit.
 * Returns the organizationId and whether the org was already active.
 */
export function completeOnboardingEffect(): Effect.Effect<
  { organizationId: string; alreadyActive: boolean },
  AuthenticationError | ConfigurationError,
  FileSystem | Keychain
> {
  return Effect.gen(function* () {
    const tokenInfo = yield* getTokenInfoEffect().pipe(
      Effect.mapError(
        (err) =>
          new ConfigurationError({
            message: `Failed to get token: ${err.message}`,
            userMessage: "Could not complete onboarding.",
          }),
      ),
    );

    if (!tokenInfo) {
      return yield* Effect.fail(
        new AuthenticationError({
          message: "No token available for onboarding",
          userMessage: "Not authenticated.",
        }),
      );
    }

    const env = yield* envGetEffect().pipe(
      Effect.orElseSucceed(() => "ote" as const),
    );
    const baseUrl = getDevxCoreUrl(env);

    const response = yield* Effect.tryPromise({
      try: () =>
        fetch(`${baseUrl}/api/v1/onboarding/cli`, {
          method: "POST",
          headers: {
            Authorization: `Bearer ${tokenInfo.accessToken}`,
            "Content-Type": "application/json",
          },
          body: JSON.stringify({}),
        }).then(async (res) => {
          if (res.status === 401) {
            throw new AuthenticationError({
              message: "CLI onboarding: unauthorized (401)",
              userMessage: "Session expired. Run 'godaddy auth login' again.",
            });
          }
          if (!res.ok) {
            const body = await res.text().catch(() => "");
            throw new ConfigurationError({
              message: `CLI onboarding failed: HTTP ${res.status} ${body}`,
              userMessage: "Could not complete onboarding.",
            });
          }
          return res.json();
        }),
      catch: (err) => {
        if (
          err instanceof AuthenticationError ||
          err instanceof ConfigurationError
        )
          return err;
        return new ConfigurationError({
          message: `CLI onboarding failed: ${err}`,
          userMessage: "Could not complete onboarding.",
        });
      },
    });

    const data = (
      response as { data?: { organizationId?: string; status?: string } }
    ).data;
    if (!data?.organizationId) {
      return yield* Effect.fail(
        new ConfigurationError({
          message: "Unexpected CLI onboarding response",
          userMessage: "Could not complete onboarding.",
        }),
      );
    }

    return {
      organizationId: data.organizationId,
      alreadyActive: data.status === "ACTIVE",
    };
  });
}
