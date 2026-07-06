import { homedir } from "node:os";
import { join } from "node:path";
import { FileSystem } from "@effect/platform/FileSystem";
import type { ArkErrors } from "arktype";
import * as Effect from "effect/Effect";
import { CliConfig } from "../cli/services/cli-config";
import { ConfigurationError, ValidationError } from "../effect/errors";
import { fileExists } from "../effect/fs-utils";
import {
  type Config,
  getConfigFileEffect as getConfigFile,
  getConfigFilePath,
} from "../services/config";

export type Environment = "ote" | "prod";

export interface EnvironmentDisplay {
  color: string;
  label: string;
}

export interface EnvironmentInfo {
  environment: Environment;
  display: EnvironmentDisplay;
  configFile?: string;
  config?: Config;
}

const ENV_FILE = ".gdenv";
const ENV_PATH = join(homedir(), ENV_FILE);
const ALL_ENVIRONMENTS: Environment[] = ["ote", "prod"];

/**
 * Test-only escape hatch. Production code reads CliConfig.environmentOverride.
 * @internal
 */
let _testEnvironmentOverride: Environment | null = null;

/** @internal For tests only. Production code uses CliConfig.environmentOverride. */
export function setRuntimeEnvironmentOverride(env: Environment | null): void {
  _testEnvironmentOverride = env;
}

function isConfigValidationErrorResult(
  value: Config | ArkErrors | null,
): value is ArkErrors {
  return typeof value === "object" && value !== null && "summary" in value;
}

/**
 * Read the environment override.
 * Priority: CliConfig service > test escape hatch > null.
 */
function getEnvironmentOverride(): Effect.Effect<
  Environment | null,
  never,
  never
> {
  return Effect.map(Effect.serviceOption(CliConfig), (option) => {
    if (option._tag === "Some" && option.value.environmentOverride) {
      return option.value.environmentOverride;
    }
    return _testEnvironmentOverride;
  });
}

/**
 * Get the current active environment (internal helper).
 * Reads override from CliConfig service, then falls back to persisted file.
 */
function getActiveEnvironmentInternalEffect(): Effect.Effect<
  Environment,
  never,
  FileSystem
> {
  return Effect.gen(function* () {
    const override = yield* getEnvironmentOverride();
    if (override) {
      return override;
    }

    const fs = yield* FileSystem;
    const exists = yield* fileExists(ENV_PATH);
    if (exists) {
      const file = yield* fs
        .readFileString(ENV_PATH)
        .pipe(Effect.orElseSucceed(() => ""));
      if (file.trim()) {
        return yield* Effect.try(() => validateEnvironment(file.trim())).pipe(
          Effect.orElseSucceed(() => "ote" as Environment),
        );
      }
    }
    return "ote" as Environment;
  });
}

/**
 * Get all available environments
 */
export function envListEffect(): Effect.Effect<
  Environment[],
  ConfigurationError,
  FileSystem
> {
  return Effect.gen(function* () {
    const activeEnv = yield* getActiveEnvironmentInternalEffect();
    const sorted = [
      activeEnv,
      ...ALL_ENVIRONMENTS.filter((e) => e !== activeEnv),
    ];
    return sorted;
  }).pipe(
    Effect.catchAll((error) =>
      Effect.fail(
        new ConfigurationError({
          message: `Failed to get environment list: ${error}`,
          userMessage: "Could not retrieve environment list",
        }),
      ),
    ),
  );
}

/**
 * Get current active environment or specific environment info
 */
export function envGetEffect(
  name?: string,
): Effect.Effect<
  Environment,
  ConfigurationError | ValidationError,
  FileSystem
> {
  return Effect.gen(function* () {
    if (name) {
      return yield* validateEnvironmentEffect(name);
    }
    return yield* getActiveEnvironmentInternalEffect();
  }).pipe(
    Effect.mapError((error): ConfigurationError | ValidationError => error),
  );
}

/**
 * Set active environment
 */
export function envSetEffect(
  name: string,
): Effect.Effect<void, ConfigurationError | ValidationError, FileSystem> {
  return Effect.gen(function* () {
    const validEnv = yield* validateEnvironmentEffect(name);
    const fs = yield* FileSystem;
    yield* fs.writeFileString(ENV_PATH, validEnv).pipe(
      Effect.mapError(
        () =>
          new ConfigurationError({
            message: `Failed to write environment file: ${ENV_PATH}`,
            userMessage: "Could not save environment setting",
          }),
      ),
    );
  }).pipe(
    Effect.mapError((error): ConfigurationError | ValidationError => error),
  );
}

/**
 * Get detailed environment information
 */
export function envInfoEffect(
  name?: string,
): Effect.Effect<
  EnvironmentInfo,
  ConfigurationError | ValidationError,
  FileSystem
> {
  return Effect.gen(function* () {
    const env = name
      ? yield* validateEnvironmentEffect(name)
      : yield* getActiveEnvironmentInternalEffect();
    const display = getEnvironmentDisplay(env);
    const configFilePath = getConfigFilePath(env);

    let config: Config | undefined;
    const configResult = yield* getConfigFile({ env }).pipe(
      Effect.catchAll(() => Effect.succeed(null)),
    );
    if (configResult && !isConfigValidationErrorResult(configResult)) {
      config = configResult;
    }

    return { environment: env, display, configFile: configFilePath, config };
  }).pipe(
    Effect.mapError((error): ConfigurationError | ValidationError => error),
  );
}

/**
 * Validate that the provided string is a valid environment
 */
export function validateEnvironment(env: string): Environment {
  const normalizedEnv = env.toLowerCase().trim();

  if (ALL_ENVIRONMENTS.includes(normalizedEnv as Environment)) {
    return normalizedEnv as Environment;
  }

  throw new ValidationError({
    message: `Invalid environment: ${env}. Must be one of: ${ALL_ENVIRONMENTS.join(", ")}`,
    userMessage: `Invalid environment: ${env}. Must be one of: ${ALL_ENVIRONMENTS.join(", ")}`,
  });
}

/**
 * Effect-based version of validateEnvironment that produces a typed error
 */
function validateEnvironmentEffect(
  env: string,
): Effect.Effect<Environment, ValidationError> {
  return Effect.try({
    try: () => validateEnvironment(env),
    catch: (e) => e as ValidationError,
  });
}

/**
 * Get the display properties for an environment
 */
export function getEnvironmentDisplay(env: Environment): EnvironmentDisplay {
  const displays: Record<Environment, EnvironmentDisplay> = {
    ote: { color: "blue", label: "OTE" },
    prod: { color: "red", label: "PROD" },
  };

  return displays[env] || displays.ote;
}

/**
 * Generate the API URL for the given environment.
 * Can be overridden with GODADDY_API_BASE_URL environment variable.
 */
export function getApiUrl(env: Environment): string {
  if (process.env.GODADDY_API_BASE_URL) {
    return process.env.GODADDY_API_BASE_URL;
  }

  if (env === "prod") {
    return "https://api.godaddy.com";
  }
  return "https://api.ote-godaddy.com";
}

/**
 * Get the OAuth Client ID for the given environment.
 * Can be overridden with GODADDY_OAUTH_CLIENT_ID environment variable.
 */
export function getClientId(env: Environment): string {
  if (process.env.GODADDY_OAUTH_CLIENT_ID) {
    return process.env.GODADDY_OAUTH_CLIENT_ID;
  }

  const clientIds: Record<Environment, string> = {
    ote: "a502484b-d7b1-4509-aa88-08b391a54c28",
    prod: "39489dee-4103-4284-9aab-9f2452142bce",
  };

  return clientIds[env];
}

/**
 * Get the devx-core API base URL for the given environment.
 * Can be overridden with DEVX_CORE_URL environment variable.
 */
export function getDevxCoreUrl(env: Environment): string {
  if (process.env.DEVX_CORE_URL) return process.env.DEVX_CORE_URL;

  const urls: Record<Environment, string> = {
    ote: "https://api.developer.commerce.ote-godaddy.com",
    prod: "https://api.developer.commerce.godaddy.com",
  };

  return urls[env];
}

/**
 * Check if an action requires confirmation in the current environment
 */
export function requiresConfirmation(
  env: Environment,
  action: "deploy" | "release" | "delete" | "update",
): boolean {
  if (env === "prod") {
    return true;
  }

  if (env === "ote" && ["deploy", "release", "delete"].includes(action)) {
    return true;
  }

  return false;
}
