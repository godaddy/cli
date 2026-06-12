import { isAbsolute, relative, resolve } from "node:path";
import type { Fetch } from "@effect/platform/FetchHttpClient";
import { FileSystem } from "@effect/platform/FileSystem";
import { type ArkErrors, type } from "arktype";
import * as Effect from "effect/Effect";
import {
  AuthenticationError,
  type CliError,
  NetworkError,
  ValidationError,
} from "../effect/errors";
import type { Keychain } from "../effect/services/keychain";
import {
  archiveApplicationEffect as archiveAppServiceEffect,
  createApplicationEffect as createAppServiceEffect,
  createReleaseEffect as createReleaseServiceEffect,
  disableApplicationEffect as disableAppServiceEffect,
  enableApplicationEffect as enableAppServiceEffect,
  getApplicationAndLatestReleaseEffect as getAppAndReleaseServiceEffect,
  getApplicationEffect as getAppServiceEffect,
  listApplicationsEffect as listAppsServiceEffect,
  updateApplicationEffect as updateAppServiceEffect,
} from "../services/applications";
import {
  type ActionConfig,
  type Config,
  type ConfigExtensionInfo,
  type SubscriptionConfig,
  createConfigFileEffect,
  createEnvFileEffect,
  getConfigFileEffect,
  getExtensionsFromConfigEffect,
} from "../services/config";
import { bundleExtensionEffect as bundleExtServiceEffect } from "../services/extension/bundler";
import { publicHttpUrl } from "../services/public-url";
import { getUploadTargetEffect } from "../services/extension/presigned-url";
import {
  scanBundleEffect,
  scanExtensionEffect,
} from "../services/extension/security-scan";
import { uploadArtifactEffect } from "../services/extension/upload";
import { getFromKeychainEffect } from "./auth";
import type { Environment } from "./environment";
import type { ScanReport } from "./security/types";

// ---------------------------------------------------------------------------
// Type definitions for core application functions
// ---------------------------------------------------------------------------

export interface ApplicationInfo {
  id: string;
  label: string;
  name: string;
  description: string;
  status: string;
  url: string;
  proxyUrl: string;
  authorizationScopes?: string[];
  releases?: Array<{
    id: string;
    version: string;
    description?: string;
    createdAt: string;
  }>;
}

export interface Application {
  id: string;
  label: string;
  name: string;
  description: string;
  status: string;
  url: string;
  proxyUrl: string;
}

export interface ValidationResult {
  valid: boolean;
  errors: string[];
  warnings: string[];
}

export interface UpdateApplicationInput {
  label?: string;
  description?: string;
  status?: "ACTIVE" | "INACTIVE";
}

export interface CreateApplicationInput {
  name: string;
  description: string;
  url: string;
  proxyUrl: string;
  authorizationScopes: string[];
}

export interface CreatedApplicationInfo {
  id: string;
  clientId: string;
  clientSecret: string;
  name: string;
  description: string;
  status: string;
  url: string;
  proxyUrl: string;
  authorizationScopes: string[];
  secret: string;
  publicKey: string;
}

export interface CreateReleaseInput {
  applicationName: string;
  version: string;
  description?: string;
  configPath?: string;
  env?: string;
}

export interface ReleaseInfo {
  id: string;
  version: string;
  description?: string;
  createdAt: string;
}

/**
 * UI extension format for release API (single target instead of array)
 */
export interface ReleaseUiExtension {
  name: string;
  handle: string;
  source: string;
  type: string;
  target?: string;
}

export interface ExtensionSecurityReport {
  extensionName: string;
  extensionDir: string;
  scannedFiles: number;
  totalFindings: number;
  blockedFindings: number;
  warnings: number;
  blocked: boolean;
  preBundleReport: ScanReport;
  postBundleReport?: ScanReport;
}

export interface ExtensionBundleReport {
  extensionName: string;
  artifactName: string;
  artifactPath: string;
  size: number;
  sha256: string;
  /** Upload IDs - one per target (or single ID if no targets) */
  uploadIds?: string[];
  /** Targets that were uploaded */
  targets?: string[];
  uploaded?: boolean;
}

export interface DeployResult {
  securityReports: ExtensionSecurityReport[];
  bundleReports: ExtensionBundleReport[];
  totalExtensions: number;
  blockedExtensions: number;
}

export interface DeployProgressEvent {
  type: "step" | "progress";
  name: string;
  status?: "started" | "completed" | "failed";
  message?: string;
  extensionName?: string;
  percent?: number;
  details?: Record<string, unknown>;
}

export interface DeployOptions {
  configPath?: string;
  env?: Environment;
  onProgress?: (event: DeployProgressEvent) => void;
}

// ---------------------------------------------------------------------------
// Input validation schemas
// ---------------------------------------------------------------------------

const updateApplicationInputValidator = type({
  label: "string?",
  description: "string?",
  status: '"ACTIVE" | "INACTIVE"?',
});

const createApplicationInputValidator = type({
  name: "string",
  description: "string",
  url: publicHttpUrl,
  proxyUrl: publicHttpUrl,
  authorizationScopes: type.string.array().moreThanLength(0),
});

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

function isConfigValidationErrorResult(
  value: Config | ArkErrors | null,
): value is ArkErrors {
  return typeof value === "object" && value !== null && "summary" in value;
}

/**
 * Retrieve a valid access token or fail with AuthenticationError.
 */
function requireAccessToken(): Effect.Effect<
  string,
  AuthenticationError,
  FileSystem | Keychain
> {
  return Effect.gen(function* () {
    const token = yield* getFromKeychainEffect("token");
    if (!token) {
      return yield* Effect.fail(
        new AuthenticationError({
          message: "Not authenticated",
          userMessage: "Please run 'godaddy auth login' first",
        }),
      );
    }
    return token;
  });
}

/**
 * Emit a deploy progress event. Best-effort and non-fatal.
 */
function emitProgress(
  options: DeployOptions | undefined,
  event: DeployProgressEvent,
): Effect.Effect<void> {
  return Effect.sync(() => {
    if (typeof options?.onProgress !== "function") {
      return;
    }
    try {
      options.onProgress(event);
    } catch {
      // Progress callbacks are best-effort and must not affect deployment.
    }
  });
}

function isPathWithin(basePath: string, candidatePath: string): boolean {
  const rel = relative(basePath, candidatePath);
  return rel === "" || (!rel.startsWith("..") && !isAbsolute(rel));
}

function resolveExtensionPathsEffect(
  repoRoot: string,
  extension: ConfigExtensionInfo,
): Effect.Effect<
  { extensionDir: string; sourcePath: string },
  ValidationError
> {
  return Effect.gen(function* () {
    const extensionsRoot = resolve(repoRoot, "extensions");
    const extensionDir = resolve(extensionsRoot, extension.handle);
    if (!isPathWithin(extensionsRoot, extensionDir)) {
      return yield* Effect.fail(
        new ValidationError({
          message: `Invalid extension handle path: ${extension.handle}`,
          userMessage:
            "Invalid extension handle path. Extension directories must stay within ./extensions.",
        }),
      );
    }

    const sourcePath = resolve(extensionDir, extension.source);
    if (!isPathWithin(extensionDir, sourcePath)) {
      return yield* Effect.fail(
        new ValidationError({
          message: `Invalid extension source path for '${extension.name}': ${extension.source}`,
          userMessage:
            "Invalid extension source path. Source files must stay within the extension directory.",
        }),
      );
    }

    return { extensionDir, sourcePath };
  });
}

/**
 * Clean up bundle artifacts (best-effort, errors ignored).
 */
function cleanupBundleArtifacts(
  artifactPath: string,
  sourcemapPath?: string,
): Effect.Effect<void, never, FileSystem> {
  return Effect.gen(function* () {
    const fs = yield* FileSystem;
    yield* fs.remove(artifactPath).pipe(Effect.orElseSucceed(() => {}));

    if (sourcemapPath) {
      yield* fs.remove(sourcemapPath).pipe(Effect.orElseSucceed(() => {}));
    }
  });
}

// ---------------------------------------------------------------------------
// Internal typed wrappers for GraphQL service calls
//
// The service layer returns complex generic types from graphql-request that
// cause `yield*` inside Effect.gen to infer `never`. These thin wrappers
// map the responses to concrete types understood by the core layer.
// ---------------------------------------------------------------------------

interface AppLookupResult {
  application: {
    id: string;
    label: string;
    name: string;
    description: string;
    status: string;
    url: string;
    proxyUrl: string;
  } | null;
}

interface AppWithReleaseLookupResult {
  application: {
    id: string;
    label: string;
    name: string;
    description: string;
    status: string;
    url: string;
    proxyUrl: string;
    authorizationScopes: string[];
    releases?: {
      edges: Array<{
        node: {
          id: string;
          version: string;
          description?: string;
          createdAt: string;
        };
      }>;
    };
  } | null;
}

interface CreateAppResult {
  createApplication: {
    id: string;
    clientId: string;
    clientSecret: string;
    name: string;
    description: string;
    status: string;
    url: string;
    proxyUrl: string;
    authorizationScopes: string[];
    secret: string;
    publicKey: string;
  } | null;
}

interface CreateReleaseResult {
  createRelease: {
    id: string;
    version: string;
    description?: string;
    createdAt: string;
  };
}

interface AppListResult {
  applications: {
    edges: Array<{
      node: {
        id: string;
        label: string;
        name: string;
        description: string;
        status: string;
        url: string;
        proxyUrl: string;
      };
    }>;
  } | null;
}

/** Narrow a service Effect's success type via Effect.map. Preserves E and R channels. */
function narrowResult<A, B, E, R>(
  effect: Effect.Effect<A, E, R>,
  f: (a: A) => B,
): Effect.Effect<B, E, R> {
  return Effect.map(effect, f);
}

function callCreateApp(
  input: Parameters<typeof createAppServiceEffect>[0],
  opts: Parameters<typeof createAppServiceEffect>[1],
) {
  return narrowResult(
    createAppServiceEffect(input, opts),
    (r) => r as CreateAppResult,
  );
}

function callGetApp(name: string, opts: { accessToken: string | null }) {
  return narrowResult(
    getAppServiceEffect(name, opts),
    (r) => r as AppLookupResult,
  );
}

function callGetAppAndRelease(
  name: string,
  opts: { accessToken: string | null },
) {
  return narrowResult(
    getAppAndReleaseServiceEffect(name, opts),
    (r) => r as AppWithReleaseLookupResult,
  );
}

function callListApps(opts: { accessToken: string | null }) {
  return narrowResult(listAppsServiceEffect(opts), (r) => r as AppListResult);
}

function callUpdateApp(
  id: string,
  input: Parameters<typeof updateAppServiceEffect>[1],
  opts: Parameters<typeof updateAppServiceEffect>[2],
) {
  return updateAppServiceEffect(id, input, opts);
}

function callArchiveApp(id: string, opts: { accessToken: string | null }) {
  return archiveAppServiceEffect(id, opts);
}

function callEnableApp(
  input: Parameters<typeof enableAppServiceEffect>[0],
  opts: Parameters<typeof enableAppServiceEffect>[1],
) {
  return enableAppServiceEffect(input, opts);
}

function callDisableApp(
  input: Parameters<typeof disableAppServiceEffect>[0],
  opts: Parameters<typeof disableAppServiceEffect>[1],
) {
  return disableAppServiceEffect(input, opts);
}

function callCreateRelease(
  input: Parameters<typeof createReleaseServiceEffect>[0],
  opts: Parameters<typeof createReleaseServiceEffect>[1],
) {
  return narrowResult(
    createReleaseServiceEffect(input, opts),
    (r) => r as CreateReleaseResult,
  );
}

// ---------------------------------------------------------------------------
// Public Effect-first API
// ---------------------------------------------------------------------------

/**
 * Initialize/create a new application.
 */
export function applicationInitEffect(
  input: CreateApplicationInput,
  environment?: Environment,
): Effect.Effect<
  CreatedApplicationInfo,
  CliError,
  FileSystem | Keychain | Fetch
> {
  return Effect.gen(function* () {
    // Validate input
    const validationResult = createApplicationInputValidator(input);
    if (validationResult instanceof type.errors) {
      return yield* Effect.fail(
        new ValidationError({
          message: validationResult.summary,
          userMessage: `Invalid application configuration: ${validationResult.summary}`,
        }),
      );
    }

    const accessToken = yield* requireAccessToken();

    // Call service function with proper format
    const createInput = {
      label: input.name,
      name: input.name,
      description: input.description,
      url: input.url,
      proxyUrl: input.proxyUrl,
      authorizationScopes: input.authorizationScopes,
    };

    const result = yield* callCreateApp(createInput, { accessToken });

    if (!result.createApplication) {
      return yield* Effect.fail(
        new NetworkError({
          message: "Failed to create application",
          userMessage: "Application creation failed - no data returned",
        }),
      );
    }

    const app = result.createApplication;
    const createdApp: CreatedApplicationInfo = {
      id: app.id,
      clientId: String(app.clientId || ""),
      clientSecret: String(app.clientSecret || ""),
      name: app.name,
      description: app.description || "",
      status: app.status,
      url: app.url,
      proxyUrl: app.proxyUrl,
      authorizationScopes: app.authorizationScopes || [],
      secret: String(app.secret || ""),
      publicKey: String(app.publicKey || ""),
    };

    // Create config and env files (best-effort, errors ignored)
    yield* createConfigFileEffect(
      {
        client_id: createdApp.clientId,
        name: createdApp.name,
        description: createdApp.description,
        url: createdApp.url,
        proxy_url: createdApp.proxyUrl,
        authorization_scopes: createdApp.authorizationScopes,
        version: "0.0.0",
        actions: [],
        subscriptions: { webhook: [] },
      },
      environment,
    ).pipe(Effect.ignore);

    yield* createEnvFileEffect(
      {
        secret: createdApp.secret,
        publicKey: createdApp.publicKey,
        clientId: createdApp.clientId,
        clientSecret: createdApp.clientSecret,
      },
      environment,
    ).pipe(Effect.ignore);

    return createdApp;
  });
}

/**
 * Get application information by name.
 */
export function applicationInfoEffect(
  name?: string,
): Effect.Effect<ApplicationInfo, CliError, FileSystem | Keychain | Fetch> {
  return Effect.gen(function* () {
    if (!name) {
      return yield* Effect.fail(
        new ValidationError({
          message: "Application name is required",
          userMessage: "Please specify an application name",
        }),
      );
    }

    const accessToken = yield* requireAccessToken();

    const result = yield* callGetAppAndRelease(name, { accessToken });

    if (!result.application) {
      return yield* Effect.fail(
        new ValidationError({
          message: `Application '${name}' not found`,
          userMessage: `Application '${name}' does not exist`,
        }),
      );
    }

    const app = result.application;
    const appInfo: ApplicationInfo = {
      id: app.id,
      label: app.label,
      name: app.name,
      description: app.description,
      status: app.status,
      url: app.url,
      proxyUrl: app.proxyUrl,
      authorizationScopes: app.authorizationScopes,
      releases: (app.releases?.edges ?? []).map((edge) => edge.node),
    };
    return appInfo;
  });
}

/**
 * List all applications.
 */
export function applicationListEffect(): Effect.Effect<
  Application[],
  CliError,
  FileSystem | Keychain | Fetch
> {
  return Effect.gen(function* () {
    const accessToken = yield* requireAccessToken();

    const result = yield* callListApps({ accessToken });

    const edges = result.applications?.edges;
    if (!edges || edges.length === 0) {
      return [] as Application[];
    }

    return edges.map(
      (edge) =>
        ({
          id: edge.node.id,
          label: edge.node.label,
          name: edge.node.name,
          description: edge.node.description,
          status: edge.node.status,
          url: edge.node.url,
          proxyUrl: edge.node.proxyUrl,
        }) satisfies Application,
    );
  });
}

/**
 * Validate application configuration.
 */
export function applicationValidateEffect(
  name?: string,
): Effect.Effect<ValidationResult, CliError, FileSystem | Keychain | Fetch> {
  return Effect.gen(function* () {
    if (!name) {
      return yield* Effect.fail(
        new ValidationError({
          message: "Application name is required",
          userMessage: "Please specify an application name",
        }),
      );
    }

    const accessToken = yield* requireAccessToken();

    const result = yield* callGetApp(name, { accessToken });

    if (!result.application) {
      return yield* Effect.fail(
        new ValidationError({
          message: `Application '${name}' not found`,
          userMessage: `Application '${name}' does not exist`,
        }),
      );
    }

    const app = result.application;
    const errors: string[] = [];
    const warnings: string[] = [];

    if (!app.url) {
      errors.push("Application URL is required");
    }
    if (!app.proxyUrl) {
      warnings.push("Proxy URL is not set");
    }
    if (app.status === "INACTIVE") {
      warnings.push("Application is currently inactive");
    }

    const validationResult: ValidationResult = {
      valid: errors.length === 0,
      errors,
      warnings,
    };
    return validationResult;
  });
}

/**
 * Update application configuration.
 */
export function applicationUpdateEffect(
  name: string,
  config: UpdateApplicationInput,
): Effect.Effect<void, CliError, FileSystem | Keychain | Fetch> {
  return Effect.gen(function* () {
    if (!name) {
      return yield* Effect.fail(
        new ValidationError({
          message: "Application name is required",
          userMessage: "Please specify an application name",
        }),
      );
    }

    // Validate input
    const validationResult = updateApplicationInputValidator(config);
    if (validationResult instanceof type.errors) {
      return yield* Effect.fail(
        new ValidationError({
          message: validationResult.summary,
          userMessage: "Invalid update configuration",
        }),
      );
    }

    const accessToken = yield* requireAccessToken();

    // Get application ID first
    const appResult = yield* callGetApp(name, { accessToken });

    if (!appResult.application) {
      return yield* Effect.fail(
        new ValidationError({
          message: `Application '${name}' not found`,
          userMessage: `Application '${name}' does not exist`,
        }),
      );
    }

    yield* callUpdateApp(appResult.application.id, config, { accessToken });
  });
}

/**
 * Enable application on a store.
 */
export function applicationEnableEffect(
  name: string,
  storeId?: string,
): Effect.Effect<void, CliError, FileSystem | Keychain | Fetch> {
  return Effect.gen(function* () {
    if (!name) {
      return yield* Effect.fail(
        new ValidationError({
          message: "Application name is required",
          userMessage: "Please specify an application name",
        }),
      );
    }

    if (!storeId) {
      return yield* Effect.fail(
        new ValidationError({
          message: "Store ID is required",
          userMessage: "Please specify a store ID",
        }),
      );
    }

    const accessToken = yield* requireAccessToken();

    yield* callEnableApp({ applicationName: name, storeId }, { accessToken });
  });
}

/**
 * Disable application on a store.
 */
export function applicationDisableEffect(
  name: string,
  storeId?: string,
): Effect.Effect<void, CliError, FileSystem | Keychain | Fetch> {
  return Effect.gen(function* () {
    if (!name) {
      return yield* Effect.fail(
        new ValidationError({
          message: "Application name is required",
          userMessage: "Please specify an application name",
        }),
      );
    }

    if (!storeId) {
      return yield* Effect.fail(
        new ValidationError({
          message: "Store ID is required",
          userMessage: "Please specify a store ID",
        }),
      );
    }

    const accessToken = yield* requireAccessToken();

    yield* callDisableApp({ applicationName: name, storeId }, { accessToken });
  });
}

/**
 * Archive application.
 */
export function applicationArchiveEffect(
  name: string,
): Effect.Effect<void, CliError, FileSystem | Keychain | Fetch> {
  return Effect.gen(function* () {
    if (!name) {
      return yield* Effect.fail(
        new ValidationError({
          message: "Application name is required",
          userMessage: "Please specify an application name",
        }),
      );
    }

    const accessToken = yield* requireAccessToken();

    // Get application ID first
    const appResult = yield* callGetApp(name, { accessToken });

    if (!appResult.application) {
      return yield* Effect.fail(
        new ValidationError({
          message: `Application '${name}' not found`,
          userMessage: `Application '${name}' does not exist`,
        }),
      );
    }

    yield* callArchiveApp(appResult.application.id, { accessToken });
  });
}

/**
 * Create a new release for an application.
 */
export function applicationReleaseEffect(
  input: CreateReleaseInput,
): Effect.Effect<ReleaseInfo, CliError, FileSystem | Keychain | Fetch> {
  return Effect.gen(function* () {
    const accessToken = yield* requireAccessToken();

    // Get application information first
    const appResult = yield* callGetApp(input.applicationName, {
      accessToken,
    });

    if (!appResult.application) {
      return yield* Effect.fail(
        new ValidationError({
          message: `Application '${input.applicationName}' not found`,
          userMessage: `Application '${input.applicationName}' does not exist`,
        }),
      );
    }

    // Load configuration to get actions, subscriptions, and extensions
    let actions: ActionConfig[] = [];
    let subscriptions: SubscriptionConfig[] = [];
    let uiExtensions: ReleaseUiExtension[] = [];

    const configResult = yield* getConfigFileEffect({
      configPath: input.configPath,
      env: input.env as Environment,
    }).pipe(Effect.orElseSucceed(() => null));

    if (configResult && !isConfigValidationErrorResult(configResult)) {
      actions = configResult.actions || [];
      subscriptions = configResult.subscriptions?.webhook || [];

      // Extract UI extensions from config
      const extensions = yield* getExtensionsFromConfigEffect({
        configPath: input.configPath,
        env: input.env as Environment,
      }).pipe(Effect.orElseSucceed(() => []));

      // Validate that only one target is specified per extension (API limitation)
      for (const ext of extensions) {
        if (ext.targets && ext.targets.length > 1) {
          yield* Effect.fail(
            new ValidationError({
              message: `UI extension "${ext.name}" has ${ext.targets.length} targets, but only one target is supported per extension during release`,
            }),
          );
        }
      }

      // Map extensions to release format
      uiExtensions = extensions.map((ext) => ({
        name: ext.name,
        handle: ext.handle,
        source: ext.source,
        type: ext.type,
        target: ext.targets?.[0]?.target,
      }));
    }

    const releaseData = {
      applicationId: appResult.application.id,
      version: input.version,
      description: input.description,
      actions,
      subscriptions,
      uiExtensions,
    };

    const result = yield* callCreateRelease(releaseData, { accessToken });

    const releaseInfo: ReleaseInfo = {
      id: result.createRelease.id,
      version: result.createRelease.version,
      description: result.createRelease.description,
      createdAt: result.createRelease.createdAt,
    };
    return releaseInfo;
  });
}

/**
 * Deploy an application (change status to ACTIVE).
 * Performs security scan, bundling, and upload before deployment.
 *
 * Prerequisites:
 * - Application must have at least one release created via `application release` command
 *
 * Flow:
 * 1. Get application and verify it exists
 * 2. Validate that application has a release
 * 3. Discover extensions in workspace
 * 4. Security scan each extension (Phase 1.5)
 * 5. Bundle each extension (Phase 2)
 * 6. Post-bundle security scan (Phase 2.5)
 * 7. Get presigned upload URLs (Phase 3)
 * 8. Upload artifacts to S3 (Phase 4)
 * 9. Update application status to ACTIVE
 */
export function applicationDeployEffect(
  applicationName: string,
  options?: DeployOptions,
): Effect.Effect<DeployResult, CliError, FileSystem | Keychain | Fetch> {
  return Effect.gen(function* () {
    yield* emitProgress(options, {
      type: "step",
      name: "deploy",
      status: "started",
      message: `Starting deployment for '${applicationName}'`,
    });

    yield* emitProgress(options, {
      type: "step",
      name: "auth.check",
      status: "started",
    });
    const accessToken = yield* requireAccessToken();
    yield* emitProgress(options, {
      type: "step",
      name: "auth.check",
      status: "completed",
    });

    // Get application and latest release
    yield* emitProgress(options, {
      type: "step",
      name: "application.lookup",
      status: "started",
    });
    const appResult = yield* callGetAppAndRelease(applicationName, {
      accessToken,
    });

    if (!appResult.application) {
      yield* emitProgress(options, {
        type: "step",
        name: "application.lookup",
        status: "failed",
        message: `Application '${applicationName}' not found`,
      });
      return yield* Effect.fail(
        new ValidationError({
          message: `Application '${applicationName}' not found`,
          userMessage: `Application '${applicationName}' does not exist`,
        }),
      );
    }
    yield* emitProgress(options, {
      type: "step",
      name: "application.lookup",
      status: "completed",
    });

    const applicationId = appResult.application.id;

    // Validate that a release exists
    yield* emitProgress(options, {
      type: "step",
      name: "release.lookup",
      status: "started",
    });
    const releases = appResult.application.releases?.edges;
    if (!releases || releases.length === 0) {
      yield* emitProgress(options, {
        type: "step",
        name: "release.lookup",
        status: "failed",
        message: "No release found for application",
      });
      return yield* Effect.fail(
        new ValidationError({
          message: "No release found for application",
          userMessage: `Application '${applicationName}' has no releases. Create a release first with: godaddy application release ${applicationName} --release-version <version>`,
        }),
      );
    }

    const latestRelease = releases[0].node;
    if (!latestRelease) {
      yield* emitProgress(options, {
        type: "step",
        name: "release.lookup",
        status: "failed",
        message: "Invalid release data",
      });
      return yield* Effect.fail(
        new ValidationError({
          message: "Invalid release data",
          userMessage: "Unable to retrieve release information",
        }),
      );
    }
    yield* emitProgress(options, {
      type: "step",
      name: "release.lookup",
      status: "completed",
    });

    const releaseId = latestRelease.id;

    // Get extensions from config file (source of truth)
    yield* emitProgress(options, {
      type: "step",
      name: "extensions.discover",
      status: "started",
    });
    const repoRoot = process.cwd();
    const extensions = yield* getExtensionsFromConfigEffect({
      configPath: options?.configPath,
      env: options?.env,
    });
    yield* emitProgress(options, {
      type: "step",
      name: "extensions.discover",
      status: "completed",
      details: { totalExtensions: extensions.length },
    });

    const securityReports: ExtensionSecurityReport[] = [];
    let blockedExtensions = 0;

    // If no extensions found, skip security scan and bundling (no-op)
    if (extensions.length === 0) {
      yield* emitProgress(options, {
        type: "step",
        name: "application.activate",
        status: "started",
      });
      yield* callUpdateApp(
        appResult.application.id,
        { status: "ACTIVE" },
        { accessToken },
      );
      yield* emitProgress(options, {
        type: "step",
        name: "application.activate",
        status: "completed",
      });
      yield* emitProgress(options, {
        type: "step",
        name: "deploy",
        status: "completed",
        details: { totalExtensions: 0, blockedExtensions: 0 },
      });

      return {
        securityReports: [],
        bundleReports: [],
        totalExtensions: 0,
        blockedExtensions: 0,
      } satisfies DeployResult;
    }

    // Scan each extension (scan the directory containing the source file)
    for (const [index, extension] of extensions.entries()) {
      const { extensionDir } = yield* resolveExtensionPathsEffect(
        repoRoot,
        extension,
      );
      yield* emitProgress(options, {
        type: "step",
        name: "scan.prebundle",
        status: "started",
        extensionName: extension.name,
        details: { extensionDir },
      });

      const report = yield* scanExtensionEffect(extensionDir).pipe(
        Effect.catchAll((err) =>
          Effect.fail(
            new ValidationError({
              message: `Security scan failed for extension '${extension.name}'`,
              userMessage: err.userMessage || "Unable to perform security scan",
            }),
          ),
        ),
      );

      yield* emitProgress(options, {
        type: "step",
        name: "scan.prebundle",
        status: "completed",
        extensionName: extension.name,
        details: {
          totalFindings: report.summary.total,
          blockedFindings: report.summary.bySeverity.block,
          warnings: report.summary.bySeverity.warn,
        },
      });
      yield* emitProgress(options, {
        type: "progress",
        name: "scan.prebundle",
        percent: Math.round(((index + 1) / extensions.length) * 100),
        message: `Scanned ${index + 1}/${extensions.length} extension(s)`,
      });

      if (report.blocked) {
        blockedExtensions++;
      }

      securityReports.push({
        extensionName: extension.name,
        extensionDir,
        scannedFiles: report.scannedFiles,
        totalFindings: report.summary.total,
        blockedFindings: report.summary.bySeverity.block,
        warnings: report.summary.bySeverity.warn,
        blocked: report.blocked,
        preBundleReport: report,
      });
    }

    // If any extension has blocking issues, fail deployment
    if (blockedExtensions > 0) {
      yield* emitProgress(options, {
        type: "step",
        name: "scan.prebundle",
        status: "failed",
        message: `${blockedExtensions} extension(s) blocked by security scan`,
        details: { blockedExtensions },
      });
      return yield* Effect.fail(
        new ValidationError({
          message: "Security violations detected",
          userMessage: `${blockedExtensions} extension(s) blocked due to security violations. Deployment blocked.`,
        }),
      );
    }

    // Bundle each extension
    const bundleReports: ExtensionBundleReport[] = [];
    const timestamp = new Date().toISOString().replace(/[:.]/g, "-");

    for (const [index, extension] of extensions.entries()) {
      const { extensionDir, sourcePath } = yield* resolveExtensionPathsEffect(
        repoRoot,
        extension,
      );
      yield* emitProgress(options, {
        type: "step",
        name: "bundle",
        status: "started",
        extensionName: extension.name,
        details: { sourcePath },
      });

      const bundle = yield* bundleExtServiceEffect(
        { name: extension.handle, version: undefined },
        sourcePath,
        {
          repoRoot,
          timestamp,
          extensionDir,
          extensionType: extension.type,
        },
      ).pipe(
        Effect.catchAll((err) =>
          Effect.fail(
            new ValidationError({
              message: `Bundle failed for extension '${extension.name}'`,
              userMessage: err.userMessage || "Unable to bundle extension",
            }),
          ),
        ),
      );

      yield* emitProgress(options, {
        type: "step",
        name: "bundle",
        status: "completed",
        extensionName: extension.name,
        details: {
          artifactName: bundle.artifactName,
          size: bundle.size,
        },
      });

      // Post-bundle security scan
      yield* emitProgress(options, {
        type: "step",
        name: "scan.postbundle",
        status: "started",
        extensionName: extension.name,
        details: { artifactName: bundle.artifactName },
      });
      const postScanReport = yield* scanBundleEffect(bundle.artifactPath).pipe(
        Effect.catchAll((err) =>
          Effect.gen(function* () {
            // On scan failure, clean up artifacts before propagating
            yield* cleanupBundleArtifacts(
              bundle.artifactPath,
              bundle.sourcemapPath,
            );
            return yield* Effect.fail(
              new ValidationError({
                message: `Post-bundle security scan failed for extension '${extension.name}'`,
                userMessage:
                  err.userMessage || "Unable to scan bundled artifact",
              }),
            );
          }),
        ),
      );

      // Cleanup and block deployment if security violations found
      if (postScanReport.blocked) {
        yield* cleanupBundleArtifacts(
          bundle.artifactPath,
          bundle.sourcemapPath,
        );
        yield* emitProgress(options, {
          type: "step",
          name: "scan.postbundle",
          status: "failed",
          extensionName: extension.name,
          message: "Security violations detected in bundled artifact",
          details: {
            totalFindings: postScanReport.summary.total,
            blockedFindings: postScanReport.summary.bySeverity.block,
          },
        });
        return yield* Effect.fail(
          new ValidationError({
            message: `Security violations detected in bundled code for extension '${extension.name}'`,
            userMessage: `${postScanReport.findings.length} security violation(s) found. Deployment blocked.`,
          }),
        );
      }
      yield* emitProgress(options, {
        type: "step",
        name: "scan.postbundle",
        status: "completed",
        extensionName: extension.name,
        details: {
          totalFindings: postScanReport.summary.total,
          blockedFindings: postScanReport.summary.bySeverity.block,
        },
      });

      const extensionSecurityReport = securityReports.find(
        (r) => r.extensionDir === extensionDir,
      );
      if (extensionSecurityReport) {
        extensionSecurityReport.postBundleReport = postScanReport;
      }

      // Get presigned upload URL(s) and upload (Phase 3 & 4)
      const targets =
        extension.type === "blocks"
          ? ["blocks"]
          : extension.targets?.length
            ? extension.targets.map((t) => t.target)
            : [undefined]; // No targets = single upload without target info

      const uploadResult = yield* Effect.gen(function* () {
        const uploadIds: string[] = [];

        yield* emitProgress(options, {
          type: "step",
          name: "upload",
          status: "started",
          extensionName: extension.name,
          details: { targetCount: targets.length },
        });

        for (const target of targets) {
          const uploadTarget = yield* getUploadTargetEffect(
            {
              applicationId,
              releaseId,
              contentType: "JS",
              target,
            },
            accessToken,
          );

          uploadIds.push(uploadTarget.uploadId);

          // Upload to S3 (Phase 4)
          yield* uploadArtifactEffect(uploadTarget, bundle.artifactPath, {
            contentType: "application/javascript",
          });
        }

        yield* emitProgress(options, {
          type: "step",
          name: "upload",
          status: "completed",
          extensionName: extension.name,
          details: { uploadCount: uploadIds.length },
        });
        yield* emitProgress(options, {
          type: "progress",
          name: "bundle.upload",
          percent: Math.round(((index + 1) / extensions.length) * 100),
          message: `Bundled and uploaded ${index + 1}/${extensions.length} extension(s)`,
        });

        return {
          uploadIds,
          uploaded: true,
        };
      }).pipe(
        Effect.tapError(() =>
          emitProgress(options, {
            type: "step",
            name: "upload",
            status: "failed",
            extensionName: extension.name,
            message: "Failed to upload extension artifact",
          }),
        ),
        Effect.ensuring(
          cleanupBundleArtifacts(bundle.artifactPath, bundle.sourcemapPath),
        ),
      );

      bundleReports.push({
        extensionName: extension.name,
        artifactName: bundle.artifactName,
        artifactPath: bundle.artifactPath,
        size: bundle.size,
        sha256: bundle.sha256,
        uploadIds: uploadResult.uploadIds,
        targets:
          extension.type === "blocks"
            ? ["blocks"]
            : extension.targets?.map((t) => t.target),
        uploaded: uploadResult.uploaded,
      });
    }

    // Update application status to ACTIVE
    yield* emitProgress(options, {
      type: "step",
      name: "application.activate",
      status: "started",
    });
    yield* callUpdateApp(
      appResult.application.id,
      { status: "ACTIVE" },
      { accessToken },
    );
    yield* emitProgress(options, {
      type: "step",
      name: "application.activate",
      status: "completed",
    });
    yield* emitProgress(options, {
      type: "step",
      name: "deploy",
      status: "completed",
      details: {
        totalExtensions: extensions.length,
        blockedExtensions,
      },
    });

    return {
      securityReports,
      bundleReports,
      totalExtensions: extensions.length,
      blockedExtensions,
    } satisfies DeployResult;
  }).pipe(
    Effect.tapError((error) =>
      emitProgress(options, {
        type: "step",
        name: "deploy",
        status: "failed",
        message:
          "userMessage" in error && typeof error.userMessage === "string"
            ? error.userMessage
            : "message" in error && typeof error.message === "string"
              ? error.message
              : "Unknown deploy error",
      }),
    ),
  );
}
