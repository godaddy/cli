import * as nodeFs from "node:fs";
import { isAbsolute, join } from "node:path";
import { FileSystem } from "@effect/platform/FileSystem";
import * as TOML from "@iarna/toml";
import { type ArkErrors, type } from "arktype";
import * as Effect from "effect/Effect";
import type { Environment } from "../core/environment";
import { ConfigurationError } from "../effect/errors";
import { fileExists } from "../effect/fs-utils";

function readProxyUrl(root: unknown): string | undefined {
  if (typeof root !== "object" || root === null) {
    return undefined;
  }

  const proxyUrl = (root as { proxy_url?: unknown }).proxy_url;
  return typeof proxyUrl === "string" ? proxyUrl : undefined;
}

const Endpoint = type("string").narrow((endpoint: string, ctx) => {
  const proxyUrl = readProxyUrl(ctx.root);
  if (!proxyUrl) {
    return ctx.mustBe("valid endpoint");
  }

  try {
    new URL(endpoint, proxyUrl);
  } catch (error) {
    return ctx.mustBe("valid endpoint");
  }

  return true;
});

const SubscriptionConfig = type({
  name: type.string.atLeastLength(3),
  events: type.string.array().atLeastLength(1),
  url: Endpoint,
});

export type SubscriptionConfig = typeof SubscriptionConfig.infer;

const SubscriptionsType = type({
  webhook: SubscriptionConfig.array(),
});

export type SubscriptionsType = typeof SubscriptionsType.infer;

const ActionConfig = type({
  name: type.string.atLeastLength(3),
  url: Endpoint,
});

export type ActionConfig = typeof ActionConfig.infer;

const DependencyConfig = type({
  name: type.string.atLeastLength(3),
  version: type.keywords.string.semver.optional(),
});

export type DependencyConfig = typeof DependencyConfig.infer;

const DependenciesType = type({
  app: DependencyConfig.array().optional(),
  feature: DependencyConfig.array().optional(),
});

export type DependenciesType = typeof DependenciesType.infer;

const ExtensionTarget = type({
  target: type.string.atLeastLength(1),
});

export type ExtensionTarget = typeof ExtensionTarget.infer;

const EmbedExtensionConfig = type({
  name: type.string.atLeastLength(3),
  handle: type.string.atLeastLength(3),
  source: type.string.atLeastLength(1),
  targets: ExtensionTarget.array().atLeastLength(1),
});

export type EmbedExtensionConfig = typeof EmbedExtensionConfig.infer;

const CheckoutExtensionConfig = type({
  name: type.string.atLeastLength(3),
  handle: type.string.atLeastLength(3),
  source: type.string.atLeastLength(1),
  targets: ExtensionTarget.array().atLeastLength(1),
});

export type CheckoutExtensionConfig = typeof CheckoutExtensionConfig.infer;

const BlocksExtensionConfig = type({
  source: type.string.atLeastLength(1),
});

export type BlocksExtensionConfig = typeof BlocksExtensionConfig.infer;

const ExtensionsType = type({
  embed: EmbedExtensionConfig.array().optional(),
  checkout: CheckoutExtensionConfig.array().optional(),
  blocks: BlocksExtensionConfig.optional(),
});

export type ExtensionsType = typeof ExtensionsType.infer;

export type ExtensionType = "embed" | "checkout" | "blocks";

/**
 * Unified extension info extracted from config for deploy operations
 */
export interface ConfigExtensionInfo {
  /** Extension type (embed, checkout, block) */
  type: ExtensionType;
  /** Extension name */
  name: string;
  /** Extension handle (unique identifier) */
  handle: string;
  /** Path to extension source file (relative to repo root) */
  source: string;
  /** Optional targets for embed/checkout extensions */
  targets?: ExtensionTarget[];
}

const Config = type({
  name: "/^[a-z0-9-]{3,255}$/",
  client_id: type.keywords.string.uuid.v4,
  description: type.string.optional(),
  version: type.keywords.string.semver,
  url: type.keywords.string.url.root,
  proxy_url: type.keywords.string.url.root,
  authorization_scopes: type.string.array().moreThanLength(0),
  subscriptions: SubscriptionsType.optional(),
  actions: ActionConfig.array().optional(),
  dependencies: DependenciesType.array().optional(),
  extensions: ExtensionsType.optional(),
});

export type Config = typeof Config.infer;
export type ConfigEnvironment = Environment | "dev" | "test";

function isConfigValidationErrors(
  value: Config | ArkErrors,
): value is ArkErrors {
  return value instanceof type.errors;
}

function toConfigError(
  error: unknown,
  fallbackMessage: string,
): ConfigurationError {
  if (error instanceof ConfigurationError) {
    return error;
  }

  if (error instanceof Error) {
    return new ConfigurationError({
      message: error.message,
      userMessage: fallbackMessage,
    });
  }

  return new ConfigurationError({
    message: fallbackMessage,
    userMessage: fallbackMessage,
  });
}

function formatEnvValue(value: string): string {
  // Always quote to prevent newline/comment/env injection in .env files.
  return JSON.stringify(value.replace(/\0/g, ""));
}

function resolveConfigEnvironment(
  env?: ConfigEnvironment,
): ConfigEnvironment | undefined {
  if (!env) {
    return undefined;
  }

  const apiOverrideCandidates = [
    process.env.APPLICATIONS_GRAPHQL_URL,
    process.env.GODADDY_API_BASE_URL,
  ].filter((value): value is string => Boolean(value?.trim()));

  for (const candidate of apiOverrideCandidates) {
    const normalizedCandidate = candidate.toLowerCase();

    if (normalizedCandidate.includes("dev-godaddy")) {
      return "dev";
    }

    if (normalizedCandidate.includes("test-godaddy")) {
      return "test";
    }
  }

  return env;
}

/**
 * Get the configuration file path based on environment
 */
export function getConfigFilePath(
  env?: ConfigEnvironment,
  configPath?: string,
): string {
  if (configPath) {
    return isAbsolute(configPath)
      ? configPath
      : join(process.cwd(), configPath);
  }

  const resolvedEnv = resolveConfigEnvironment(env);
  const fileName = resolvedEnv ? `godaddy.${resolvedEnv}.toml` : "godaddy.toml";
  return join(process.cwd(), fileName);
}

// ---------------------------------------------------------------------------
// Effectful config file operations (platform FileSystem)
// ---------------------------------------------------------------------------

/**
 * Read and parse a config file. Returns Config or ArkErrors.
 */
export function getConfigFileEffect({
  configPath,
  env,
}: {
  configPath?: string;
  env?: ConfigEnvironment;
} = {}): Effect.Effect<Config | ArkErrors, ConfigurationError, FileSystem> {
  return Effect.gen(function* () {
    const fs = yield* FileSystem;
    const resolvedEnv = resolveConfigEnvironment(env);

    // If a specific config path is provided, use that
    if (configPath) {
      const absolutePath = getConfigFilePath(undefined, configPath);
      const exists = yield* fileExists(absolutePath);
      if (exists) {
        const content = yield* fs.readFileString(absolutePath);
        return Config(TOML.parse(content));
      }
      return yield* Effect.fail(
        new ConfigurationError({
          message: `Config file not found at ${absolutePath}`,
          userMessage: `Config file not found at ${absolutePath}`,
        }),
      );
    }

    // Try environment-specific file first
    if (resolvedEnv) {
      const envFilePath = getConfigFilePath(resolvedEnv);
      const envExists = yield* fileExists(envFilePath);
      if (envExists) {
        const content = yield* fs.readFileString(envFilePath);
        return Config(TOML.parse(content));
      }
    }

    // Fall back to default config file
    const defaultPath = getConfigFilePath();
    const defaultExists = yield* fileExists(defaultPath);
    if (defaultExists) {
      const content = yield* fs.readFileString(defaultPath);
      return Config(TOML.parse(content));
    }

    const envHint =
      resolvedEnv && resolvedEnv !== "prod"
        ? ` Consider running 'godaddy application init' to create environment-specific configs.`
        : "";
    return yield* Effect.fail(
      new ConfigurationError({
        message: `Config file not found at ${defaultPath}.${envHint}`,
        userMessage: `Config file not found at ${defaultPath}.${envHint}`,
      }),
    );
  }).pipe(
    Effect.catchAll((error) =>
      Effect.fail(toConfigError(error, "Failed to read config file")),
    ),
  );
}

/**
 * Backward-compatible sync version using node:fs directly.
 * Only for non-Effect call sites (tests, etc.). Prefer getConfigFileEffect.
 */
export function getConfigFile({
  configPath,
  env,
}: {
  configPath?: string;
  env?: ConfigEnvironment;
} = {}): Config | ArkErrors {
  const resolvedEnv = resolveConfigEnvironment(env);

  if (configPath) {
    const absolutePath = getConfigFilePath(undefined, configPath);
    if (nodeFs.existsSync(absolutePath)) {
      const content = nodeFs.readFileSync(absolutePath, "utf-8");
      return Config(TOML.parse(content));
    }
    throw new Error(`Config file not found at ${absolutePath}`);
  }

  if (resolvedEnv) {
    const envFilePath = getConfigFilePath(resolvedEnv);
    if (nodeFs.existsSync(envFilePath)) {
      const content = nodeFs.readFileSync(envFilePath, "utf-8");
      return Config(TOML.parse(content));
    }
  }

  const defaultPath = getConfigFilePath();
  if (nodeFs.existsSync(defaultPath)) {
    const content = nodeFs.readFileSync(defaultPath, "utf-8");
    return Config(TOML.parse(content));
  }

  const envHint =
    resolvedEnv && resolvedEnv !== "prod"
      ? ` Consider running 'godaddy application init' to create environment-specific configs.`
      : "";
  throw new Error(`Config file not found at ${defaultPath}.${envHint}`);
}

/**
 * Extract all extensions from config file as a flat array.
 */
export function getExtensionsFromConfigEffect(
  options: { configPath?: string; env?: Environment } = {},
): Effect.Effect<ConfigExtensionInfo[], ConfigurationError, FileSystem> {
  return Effect.gen(function* () {
    const config = yield* getConfigFileEffect(options).pipe(
      Effect.catchAll(() => Effect.succeed(null)),
    );

    if (!config || isConfigValidationErrors(config)) {
      return [];
    }

    return extractExtensionsFromConfig(config);
  });
}

/**
 * Backward-compatible sync version.
 */
export function getExtensionsFromConfig(
  options: { configPath?: string; env?: Environment } = {},
): ConfigExtensionInfo[] {
  const config = getConfigFile(options);
  if (isConfigValidationErrors(config)) {
    return [];
  }
  return extractExtensionsFromConfig(config);
}

function extractExtensionsFromConfig(config: Config): ConfigExtensionInfo[] {
  const extensions: ConfigExtensionInfo[] = [];

  if (config.extensions?.embed) {
    for (const ext of config.extensions.embed) {
      if (!ext.name || !ext.handle || !ext.source) {
        throw new Error(
          "Invalid embed extension config: missing required fields (name, handle, source)",
        );
      }
      extensions.push({
        type: "embed",
        name: ext.name,
        handle: ext.handle,
        source: ext.source,
        targets: ext.targets,
      });
    }
  }

  if (config.extensions?.checkout) {
    for (const ext of config.extensions.checkout) {
      if (!ext.name || !ext.handle || !ext.source) {
        throw new Error(
          "Invalid checkout extension config: missing required fields (name, handle, source)",
        );
      }
      extensions.push({
        type: "checkout",
        name: ext.name,
        handle: ext.handle,
        source: ext.source,
        targets: ext.targets,
      });
    }
  }

  if (config.extensions?.blocks) {
    const blocks = config.extensions.blocks;
    if (!blocks.source) {
      throw new Error(
        "Invalid blocks extension config: missing required 'source' field",
      );
    }
    extensions.push({
      type: "blocks",
      name: "Blocks",
      handle: "blocks",
      source: blocks.source,
    });
  }

  return extensions;
}

/**
 * Determine which config file path to use for updates
 */
function getConfigFilePathForUpdateEffect(
  configPath?: string,
  env?: ConfigEnvironment,
): Effect.Effect<
  { path: string; env?: ConfigEnvironment },
  ConfigurationError,
  FileSystem
> {
  return Effect.gen(function* () {
    const resolvedEnv = resolveConfigEnvironment(env);

    if (configPath) {
      const absolutePath = getConfigFilePath(undefined, configPath);
      const exists = yield* fileExists(absolutePath);
      if (exists) {
        return { path: absolutePath };
      }
      return yield* Effect.fail(
        new ConfigurationError({
          message: `Config file not found at ${absolutePath}`,
          userMessage: `Config file not found at ${absolutePath}`,
        }),
      );
    }

    if (resolvedEnv) {
      const envFilePath = getConfigFilePath(resolvedEnv);
      const exists = yield* fileExists(envFilePath);
      if (exists) {
        return { path: envFilePath, env: resolvedEnv };
      }
    }

    const defaultPath = getConfigFilePath();
    const exists = yield* fileExists(defaultPath);
    if (exists) {
      return { path: defaultPath };
    }

    if (resolvedEnv) {
      return { path: getConfigFilePath(resolvedEnv), env: resolvedEnv };
    }

    return { path: defaultPath };
  });
}

/**
 * Write a full Config object to the TOML file.
 */
function writeConfigToResolvedPathEffect(
  data: Config,
  filePath: string,
): Effect.Effect<void, ConfigurationError, FileSystem> {
  return Effect.gen(function* () {
    const fs = yield* FileSystem;

    // Try to read the existing file to preserve structure
    let existingConfig = {};
    const configFileExists = yield* fileExists(filePath);
    if (configFileExists) {
      const existingContent = yield* fs
        .readFileString(filePath)
        .pipe(Effect.orElseSucceed(() => ""));
      if (existingContent) {
        try {
          existingConfig = TOML.parse(existingContent);
        } catch {
          // Can't parse, use empty object
        }
      }
    }

    const formattedActions = data.actions?.map((action) => {
      if (typeof action === "string") {
        return { name: action, url: "" };
      }
      return action;
    });

    const tomlData: Record<string, unknown> = {
      ...Object.fromEntries(
        Object.entries(existingConfig as Record<string, unknown>).filter(
          ([key]) =>
            ![
              "name",
              "client_id",
              "description",
              "version",
              "url",
              "proxy_url",
              "authorization_scopes",
              "actions",
              "subscriptions",
              "default",
            ].includes(key),
        ),
      ),
      name: data.name,
      client_id: data.client_id,
      description: data.description || "",
      version: data.version,
      url: data.url,
      proxy_url: data.proxy_url,
      authorization_scopes: data.authorization_scopes || [],
      actions: formattedActions,
      subscriptions: data.subscriptions,
    };

    if ("default" in existingConfig) {
      tomlData.default = (existingConfig as Record<string, unknown>).default;
    }

    if (data.dependencies) {
      tomlData.dependencies = data.dependencies;
    }

    if (data.extensions) {
      tomlData.extensions = data.extensions;
    }

    const cleanedTomlData = Object.entries(tomlData).reduce(
      (acc, [key, value]) => {
        if (value !== undefined) {
          acc[key] = value as TOML.AnyJson;
        }
        return acc;
      },
      {} as TOML.JsonMap,
    );

    const tomlString = TOML.stringify(cleanedTomlData);
    yield* fs.writeFileString(filePath, tomlString);
  }).pipe(
    Effect.catchAll((error) =>
      Effect.fail(toConfigError(error, "Failed to write config file")),
    ),
  );
}

function writeConfigToFileEffect(
  data: Config,
  env?: ConfigEnvironment,
): Effect.Effect<void, ConfigurationError, FileSystem> {
  return writeConfigToResolvedPathEffect(data, getConfigFilePath(env));
}

/**
 * Write the config data to the appropriate TOML file.
 */
export function createConfigFileEffect(
  data: Config,
  env?: ConfigEnvironment,
): Effect.Effect<void, ConfigurationError, FileSystem> {
  return writeConfigToFileEffect(data, env);
}

/**
 * Update the version number in the config file.
 */
export function updateVersionNumberEffect(
  version: string | null,
): Effect.Effect<void, ConfigurationError, FileSystem> {
  return Effect.gen(function* () {
    if (!version) return;

    const config = yield* getConfigFileEffect({});
    if (isConfigValidationErrors(config)) {
      return yield* Effect.fail(
        new ConfigurationError({
          message: "Config file validation failed",
          userMessage: config.summary,
        }),
      );
    }
    const newConfig = { ...config, version };
    yield* writeConfigToFileEffect(newConfig);
  });
}

/**
 * Add an action to the config file.
 */
export function addActionToConfigEffect(
  action: ActionConfig,
  options: { configPath?: string; env?: Environment } = {},
): Effect.Effect<void, ConfigurationError, FileSystem> {
  return Effect.gen(function* () {
    const configResult = yield* getConfigFileEffect(options);
    if (isConfigValidationErrors(configResult)) {
      return yield* Effect.fail(
        new ConfigurationError({
          message: "Config file validation failed",
          userMessage: configResult.summary,
        }),
      );
    }

    const updatedConfig: Config = {
      ...configResult,
      actions: [...(configResult.actions || []), action],
    };

    const { path } = yield* getConfigFilePathForUpdateEffect(
      options.configPath,
      options.env,
    );
    yield* writeConfigToResolvedPathEffect(updatedConfig, path);
  }).pipe(
    Effect.catchAll((error) =>
      Effect.fail(toConfigError(error, "Unable to update actions in config")),
    ),
  );
}

/**
 * Add a subscription to the config file.
 */
export function addSubscriptionToConfigEffect(
  subscription: SubscriptionConfig,
  options: { configPath?: string; env?: Environment } = {},
): Effect.Effect<void, ConfigurationError, FileSystem> {
  return Effect.gen(function* () {
    const configResult = yield* getConfigFileEffect(options);
    if (isConfigValidationErrors(configResult)) {
      return yield* Effect.fail(
        new ConfigurationError({
          message: "Config file validation failed",
          userMessage: configResult.summary,
        }),
      );
    }

    const updatedConfig: Config = {
      ...configResult,
      subscriptions: {
        webhook: [...(configResult.subscriptions?.webhook || []), subscription],
      },
    };

    const { path } = yield* getConfigFilePathForUpdateEffect(
      options.configPath,
      options.env,
    );
    yield* writeConfigToResolvedPathEffect(updatedConfig, path);
  }).pipe(
    Effect.catchAll((error) =>
      Effect.fail(
        toConfigError(error, "Unable to update subscriptions in config"),
      ),
    ),
  );
}

/**
 * Create a .env file with application secrets.
 */
export function createEnvFileEffect(
  {
    secret,
    publicKey,
    clientId,
    clientSecret,
  }: {
    secret: string;
    publicKey: string;
    clientId: string;
    clientSecret: string;
  },
  env?: ConfigEnvironment,
): Effect.Effect<void, ConfigurationError, FileSystem> {
  return Effect.gen(function* () {
    const fs = yield* FileSystem;
    const resolvedEnv = resolveConfigEnvironment(env);
    const envFileName = resolvedEnv ? `.env.${resolvedEnv}` : ".env";
    const envPath = join(process.cwd(), envFileName);

    let envContent = "";
    const envExists = yield* fileExists(envPath);

    if (envExists) {
      const existingEnvContent = yield* fs
        .readFileString(envPath)
        .pipe(Effect.orElseSucceed(() => ""));

      if (existingEnvContent) {
        const envLines = existingEnvContent.split("\n");
        const envVars: Record<string, string> = {};

        for (const line of envLines) {
          if (line.trim() && !line.startsWith("#")) {
            const [key, ...valueParts] = line.split("=");
            if (key) {
              envVars[key.trim()] = valueParts.join("=").trim();
            }
          }
        }

        envVars.GODADDY_WEBHOOK_SECRET = formatEnvValue(secret);
        envVars.GODADDY_PUBLIC_KEY = formatEnvValue(publicKey);
        envVars.GODADDY_CLIENT_ID = formatEnvValue(clientId);
        envVars.GODADDY_CLIENT_SECRET = formatEnvValue(clientSecret);

        envContent = Object.entries(envVars)
          .map(([key, value]) => `${key}=${value}`)
          .join("\n");

        for (const line of envLines) {
          if (line.trim() && (line.startsWith("#") || !line.includes("="))) {
            envContent += `\n${line}`;
          }
        }
      } else {
        envContent = `GODADDY_WEBHOOK_SECRET=${formatEnvValue(secret)}\nGODADDY_PUBLIC_KEY=${formatEnvValue(publicKey)}\nGODADDY_CLIENT_ID=${formatEnvValue(clientId)}\nGODADDY_CLIENT_SECRET=${formatEnvValue(clientSecret)}`;
      }
    } else {
      envContent = `GODADDY_WEBHOOK_SECRET=${formatEnvValue(secret)}\nGODADDY_PUBLIC_KEY=${formatEnvValue(publicKey)}\nGODADDY_CLIENT_ID=${formatEnvValue(clientId)}\nGODADDY_CLIENT_SECRET=${formatEnvValue(clientSecret)}`;
    }

    yield* fs.writeFileString(envPath, envContent);
  }).pipe(
    Effect.catchAll((error) =>
      Effect.fail(toConfigError(error, "Failed to create .env file")),
    ),
  );
}

/**
 * Add an extension to the config file.
 */
export function addExtensionToConfigEffect(
  extensionType: ExtensionType,
  extension:
    | EmbedExtensionConfig
    | CheckoutExtensionConfig
    | BlocksExtensionConfig,
  options: { configPath?: string; env?: Environment } = {},
): Effect.Effect<void, ConfigurationError, FileSystem> {
  return Effect.gen(function* () {
    const configResult = yield* getConfigFileEffect(options);
    if (isConfigValidationErrors(configResult)) {
      return yield* Effect.fail(
        new ConfigurationError({
          message: "Config file validation failed",
          userMessage: configResult.summary,
        }),
      );
    }

    const currentExtensions = configResult.extensions || {};
    let updatedExtensions: ExtensionsType;

    if (extensionType === "blocks") {
      updatedExtensions = {
        ...currentExtensions,
        blocks: extension as BlocksExtensionConfig,
      };
    } else {
      updatedExtensions = {
        ...currentExtensions,
        [extensionType]: [
          ...((currentExtensions[extensionType] as Array<unknown>) || []),
          extension,
        ],
      } as ExtensionsType;
    }

    const updatedConfig = {
      ...configResult,
      extensions: updatedExtensions,
    } satisfies Config;

    const { path } = yield* getConfigFilePathForUpdateEffect(
      options.configPath,
      options.env,
    );
    yield* writeConfigToResolvedPathEffect(updatedConfig, path);
  }).pipe(
    Effect.catchAll((error) =>
      Effect.fail(
        toConfigError(error, "Unable to update extensions in config"),
      ),
    ),
  );
}
