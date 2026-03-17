import { join, resolve } from "node:path";
import * as Args from "@effect/cli/Args";
import * as Command from "@effect/cli/Command";
import * as Options from "@effect/cli/Options";
import type { ArkErrors } from "arktype";
import * as Effect from "effect/Effect";
import * as Option from "effect/Option";
import type {
  CreateApplicationInput,
  DeployProgressEvent,
  DeployResult,
} from "../../core/applications";
import {
  applicationArchiveEffect,
  applicationDeployEffect,
  applicationDisableEffect,
  applicationEnableEffect,
  applicationInfoEffect,
  applicationInitEffect,
  applicationListEffect,
  applicationReleaseEffect,
  applicationUpdateEffect,
  applicationValidateEffect,
} from "../../core/applications";
import { type Environment, envGetEffect } from "../../core/environment";
import { ValidationError } from "../../effect/errors";
import {
  type ActionConfig,
  type BlocksExtensionConfig,
  type CheckoutExtensionConfig,
  type Config,
  type EmbedExtensionConfig,
  type SubscriptionConfig,
  addActionToConfigEffect,
  addExtensionToConfigEffect,
  addSubscriptionToConfigEffect,
  getConfigFile,
  getConfigFilePath,
} from "../../services/config";
import { protectPayload, truncateList } from "../agent/truncation";
import type { NextAction } from "../agent/types";
import { EnvelopeWriter } from "../services/envelope-writer";
// ---------------------------------------------------------------------------
// Helpers (pure, no global state)
// ---------------------------------------------------------------------------

type ConfigReadResult = ReturnType<typeof getConfigFile>;

function resolveEnvironmentEffect(environment?: string) {
  return envGetEffect(environment);
}

function resolveConfigPath(
  configPath: string | undefined,
  env: Environment,
): string {
  if (configPath) return resolve(process.cwd(), configPath);
  return getConfigFilePath(env);
}

function parseSpaceSeparated(value: string): string[] {
  return value
    .split(" ")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

function parseCommaSeparated(value: string): string[] {
  return value
    .split(",")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

function isConfigValidationErrorResult(
  value: ConfigReadResult,
): value is ArkErrors {
  return typeof value === "object" && value !== null && "summary" in value;
}

function buildDeployPayload(
  name: string,
  deployResult: DeployResult,
): Record<string, unknown> {
  const summarized = protectPayload(
    {
      total_extensions: deployResult.totalExtensions,
      blocked_extensions: deployResult.blockedExtensions,
      security_reports: deployResult.securityReports.map((r) => ({
        extension_name: r.extensionName,
        extension_dir: r.extensionDir,
        blocked: r.blocked,
        total_findings: r.totalFindings,
        blocked_findings: r.blockedFindings,
        warnings: r.warnings,
        pre_bundle: {
          blocked: r.preBundleReport.blocked,
          scanned_files: r.preBundleReport.scannedFiles,
          summary: r.preBundleReport.summary,
          findings: r.preBundleReport.findings,
        },
        post_bundle: r.postBundleReport
          ? {
              blocked: r.postBundleReport.blocked,
              scanned_files: r.postBundleReport.scannedFiles,
              summary: r.postBundleReport.summary,
              findings: r.postBundleReport.findings,
            }
          : undefined,
      })),
      bundle_reports: deployResult.bundleReports.map((r) => ({
        extension_name: r.extensionName,
        artifact_name: r.artifactName,
        size_bytes: r.size,
        sha256: r.sha256,
        targets: r.targets,
        upload_ids: r.uploadIds,
        uploaded: r.uploaded,
      })),
    },
    `application-deploy-${name}`,
  );
  return {
    ...summarized.value,
    truncated: summarized.metadata?.truncated ?? false,
    total: summarized.metadata?.total,
    shown: summarized.metadata?.shown,
    full_output: summarized.metadata?.full_output,
  };
}

// ---------------------------------------------------------------------------
// Colocated next_actions
// ---------------------------------------------------------------------------

const appGroupActions: NextAction[] = [
  { command: "godaddy application list", description: "List all applications" },
  {
    command:
      "godaddy application init --name <name> --description <description> --url <url> --proxy-url <proxyUrl> --scopes <scopes>",
    description: "Initialize a new application",
  },
  {
    command: "godaddy application add action --name <name> --url <url>",
    description: "Add action configuration",
  },
];

function appInfoActions(name: string): NextAction[] {
  return [
    {
      command: "godaddy application validate <name>",
      description: "Validate application configuration",
      params: { name: { value: name, required: true } },
    },
    {
      command:
        "godaddy application update <name> [--label <label>] [--description <description>] [--status <status>]",
      description: "Update application configuration",
      params: {
        name: { value: name, required: true },
        status: { enum: ["ACTIVE", "INACTIVE"] },
      },
    },
    {
      command: "godaddy application release <name> --release-version <version>",
      description: "Create a release",
      params: {
        name: { value: name, required: true },
        version: { required: true },
      },
    },
  ];
}

const appListActions: NextAction[] = [
  {
    command: "godaddy application info <name>",
    description: "Get details for a specific application",
    params: { name: { required: true } },
  },
  {
    command:
      "godaddy application init --name <name> --description <description> --url <url> --proxy-url <proxyUrl> --scopes <scopes>",
    description: "Initialize a new application",
  },
];

function appValidateActions(name: string): NextAction[] {
  return [
    {
      command: "godaddy application release <name> --release-version <version>",
      description: "Create a release after validation",
      params: {
        name: { value: name, required: true },
        version: { required: true },
      },
    },
    {
      command: "godaddy application info <name>",
      description: "Inspect application details",
      params: { name: { value: name, required: true } },
    },
  ];
}

function appUpdateActions(name: string): NextAction[] {
  return [
    {
      command: "godaddy application info <name>",
      description: "Inspect updated application",
      params: { name: { value: name, required: true } },
    },
    {
      command: "godaddy application deploy <name>",
      description: "Deploy updated application",
      params: { name: { value: name, required: true } },
    },
  ];
}

function appEnableActions(name: string, storeId: string): NextAction[] {
  return [
    {
      command: "godaddy application disable <name> --store-id <storeId>",
      description: "Disable the application on the same store",
      params: {
        name: { value: name, required: true },
        storeId: { value: storeId, required: true },
      },
    },
    {
      command: "godaddy application info <name>",
      description: "Inspect application status",
      params: { name: { value: name, required: true } },
    },
  ];
}

function appDisableActions(name: string, storeId: string): NextAction[] {
  return [
    {
      command: "godaddy application enable <name> --store-id <storeId>",
      description: "Re-enable the application on the same store",
      params: {
        name: { value: name, required: true },
        storeId: { value: storeId, required: true },
      },
    },
    {
      command: "godaddy application info <name>",
      description: "Inspect application status",
      params: { name: { value: name, required: true } },
    },
  ];
}

function appArchiveActions(name: string): NextAction[] {
  return [
    {
      command: "godaddy application info <name>",
      description: "Inspect archived application",
      params: { name: { value: name, required: true } },
    },
    {
      command: "godaddy application list",
      description: "List all applications",
    },
  ];
}

function appInitActions(name: string): NextAction[] {
  return [
    {
      command: "godaddy application add action --name <name> --url <url>",
      description: "Add first action",
    },
    {
      command:
        "godaddy application add subscription --name <name> --events <events> --url <url>",
      description: "Add webhook subscription",
    },
    {
      command: "godaddy application release <name> --release-version <version>",
      description: "Create first release",
      params: {
        name: { value: name, required: true },
        version: { required: true },
      },
    },
  ];
}

const addConfigActions: NextAction[] = [
  {
    command: "godaddy application validate <name>",
    description: "Validate application configuration",
    params: { name: { required: true } },
  },
  {
    command: "godaddy application release <name> --release-version <version>",
    description: "Create a new release",
    params: { name: { required: true }, version: { required: true } },
  },
];

function appReleaseActions(name: string): NextAction[] {
  return [
    {
      command: "godaddy application deploy <name> [--follow]",
      description: "Deploy the released application",
      params: {
        name: { value: name, required: true },
        follow: {
          description: "Stream deploy progress as NDJSON events",
          default: false,
        },
      },
    },
    {
      command: "godaddy application info <name>",
      description: "Inspect application and latest release",
      params: { name: { value: name, required: true } },
    },
  ];
}

function appDeployActions(name: string): NextAction[] {
  return [
    {
      command: "godaddy application enable <name> --store-id <storeId>",
      description: "Enable the application on a store",
      params: {
        name: { value: name, required: true },
        storeId: { required: true },
      },
    },
    {
      command: "godaddy application info <name>",
      description: "Inspect deployment status",
      params: { name: { value: name, required: true } },
    },
    {
      command: "godaddy application deploy <name> [--follow]",
      description: "Rerun deployment with optional NDJSON progress stream",
      params: {
        name: { value: name, required: true },
        follow: {
          description: "Stream deploy progress as NDJSON events",
          default: false,
        },
      },
    },
  ];
}

const addGroupActions: NextAction[] = [
  {
    command: "godaddy application add action --name <name> --url <url>",
    description: "Add action configuration",
  },
  {
    command:
      "godaddy application add subscription --name <name> --events <events> --url <url>",
    description: "Add webhook subscription",
  },
  {
    command: "godaddy application add extension",
    description: "Show extension add commands",
  },
];

const addExtGroupActions: NextAction[] = [
  {
    command:
      "godaddy application add extension embed --name <name> --handle <handle> --source <source> --target <targets>",
    description: "Add embed extension",
  },
  {
    command:
      "godaddy application add extension checkout --name <name> --handle <handle> --source <source> --target <targets>",
    description: "Add checkout extension",
  },
  {
    command: "godaddy application add extension blocks --source <source>",
    description: "Configure blocks extension",
  },
];

// ---------------------------------------------------------------------------
// Common option sets
// ---------------------------------------------------------------------------

const configOption = Options.text("config").pipe(
  Options.withAlias("c"),
  Options.withDescription("Path to configuration file"),
  Options.optional,
);
const envOption = Options.text("environment").pipe(
  Options.withDescription("Environment (ote|prod)"),
  Options.optional,
);

// ---------------------------------------------------------------------------
// Leaf commands
// ---------------------------------------------------------------------------

const appInfo = Command.make(
  "info",
  {
    name: Args.text({ name: "name" }).pipe(
      Args.withDescription("Application name"),
    ),
  },
  ({ name }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const appData = yield* applicationInfoEffect(name);
      const latestRelease = appData.releases?.[0]
        ? {
            id: appData.releases[0].id,
            version: appData.releases[0].version,
            description: appData.releases[0].description,
            created_at: appData.releases[0].createdAt,
          }
        : null;
      yield* writer.emitSuccess(
        "godaddy application info",
        {
          id: appData.id,
          label: appData.label,
          name: appData.name,
          description: appData.description,
          status: appData.status,
          url: appData.url,
          proxy_url: appData.proxyUrl,
          authorization_scopes: appData.authorizationScopes ?? [],
          latest_release: latestRelease,
        },
        appInfoActions(name),
      );
    }),
).pipe(Command.withDescription("Show application information"));

const appList = Command.make("list", {}, () =>
  Effect.gen(function* () {
    const writer = yield* EnvelopeWriter;
    const applications = yield* applicationListEffect();
    const truncated = truncateList(applications, "application-list");
    yield* writer.emitSuccess(
      "godaddy application list",
      {
        applications: truncated.items,
        total: truncated.metadata.total,
        shown: truncated.metadata.shown,
        truncated: truncated.metadata.truncated,
        full_output: truncated.metadata.full_output,
      },
      appListActions,
    );
  }),
).pipe(Command.withDescription("List all applications"));

const appValidate = Command.make(
  "validate",
  {
    name: Args.text({ name: "name" }).pipe(
      Args.withDescription("Application name"),
    ),
  },
  ({ name }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const validation = yield* applicationValidateEffect(name);
      const pp = protectPayload(
        {
          valid: validation.valid,
          errors: validation.errors,
          warnings: validation.warnings,
        },
        `application-validate-${name}`,
      );
      yield* writer.emitSuccess(
        "godaddy application validate",
        {
          valid: validation.valid,
          error_count: validation.errors.length,
          warning_count: validation.warnings.length,
          details: pp.value,
          truncated: pp.metadata?.truncated ?? false,
          total: pp.metadata?.total,
          shown: pp.metadata?.shown,
          full_output: pp.metadata?.full_output,
        },
        appValidateActions(name),
      );
    }),
).pipe(Command.withDescription("Validate application configuration"));

const appUpdate = Command.make(
  "update",
  {
    name: Args.text({ name: "name" }).pipe(
      Args.withDescription("Application name"),
    ),
    label: Options.text("label").pipe(
      Options.withDescription("Application label"),
      Options.optional,
    ),
    description: Options.text("description").pipe(
      Options.withDescription("Application description"),
      Options.optional,
    ),
    status: Options.text("status").pipe(
      Options.withDescription("Application status (ACTIVE|INACTIVE)"),
      Options.optional,
    ),
  },
  ({ name, label, description, status }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const config: {
        label?: string;
        description?: string;
        status?: "ACTIVE" | "INACTIVE";
      } = {};
      const lbl = Option.getOrUndefined(label);
      const desc = Option.getOrUndefined(description);
      const st = Option.getOrUndefined(status);
      if (lbl) config.label = lbl;
      if (desc) config.description = desc;
      if (st) {
        if (st !== "ACTIVE" && st !== "INACTIVE") {
          return yield* Effect.fail(
            new ValidationError({
              message: "Status must be either ACTIVE or INACTIVE",
              userMessage: "Status must be either ACTIVE or INACTIVE",
            }),
          );
        }
        config.status = st;
      }
      if (Object.keys(config).length === 0) {
        return yield* Effect.fail(
          new ValidationError({
            message: "At least one field must be specified for update",
            userMessage: "Provide one of: --label, --description, --status",
          }),
        );
      }
      yield* applicationUpdateEffect(name, config);
      yield* writer.emitSuccess(
        "godaddy application update",
        { name, updated_fields: Object.keys(config), status: config.status },
        appUpdateActions(name),
      );
    }),
).pipe(Command.withDescription("Update application configuration"));

const appEnable = Command.make(
  "enable",
  {
    name: Args.text({ name: "name" }).pipe(
      Args.withDescription("Application name"),
    ),
    storeId: Options.text("store-id").pipe(Options.withDescription("Store ID")),
  },
  ({ name, storeId }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      yield* applicationEnableEffect(name, storeId);
      yield* writer.emitSuccess(
        "godaddy application enable",
        { name, store_id: storeId, enabled: true },
        appEnableActions(name, storeId),
      );
    }),
).pipe(Command.withDescription("Enable application on a store"));

const appDisable = Command.make(
  "disable",
  {
    name: Args.text({ name: "name" }).pipe(
      Args.withDescription("Application name"),
    ),
    storeId: Options.text("store-id").pipe(Options.withDescription("Store ID")),
  },
  ({ name, storeId }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      yield* applicationDisableEffect(name, storeId);
      yield* writer.emitSuccess(
        "godaddy application disable",
        { name, store_id: storeId, enabled: false },
        appDisableActions(name, storeId),
      );
    }),
).pipe(Command.withDescription("Disable application on a store"));

const appArchive = Command.make(
  "archive",
  {
    name: Args.text({ name: "name" }).pipe(
      Args.withDescription("Application name"),
    ),
  },
  ({ name }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      yield* applicationArchiveEffect(name);
      yield* writer.emitSuccess(
        "godaddy application archive",
        { name, archived: true },
        appArchiveActions(name),
      );
    }),
).pipe(Command.withDescription("Archive application"));

const appInit = Command.make(
  "init",
  {
    name: Options.text("name").pipe(
      Options.withDescription("Application name"),
      Options.optional,
    ),
    description: Options.text("description").pipe(
      Options.withDescription("Application description"),
      Options.optional,
    ),
    url: Options.text("url").pipe(
      Options.withDescription("Application URL"),
      Options.optional,
    ),
    proxyUrl: Options.text("proxy-url").pipe(
      Options.withDescription("Proxy URL for API endpoints"),
      Options.optional,
    ),
    scopes: Options.text("scopes").pipe(
      Options.withDescription("Authorization scopes (space-separated)"),
      Options.repeated,
    ),
    config: configOption,
    environment: envOption,
  },
  (opts) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const cfgPath = Option.getOrUndefined(opts.config);
      const envStr = Option.getOrUndefined(opts.environment);
      let cfg: Config | undefined;
      if (cfgPath || envStr) {
        const candidate = getConfigFile({
          configPath: cfgPath,
          env: envStr as Environment | undefined,
        });
        if (isConfigValidationErrorResult(candidate)) {
          const problems =
            typeof candidate.summary === "string"
              ? candidate.summary
              : "Config file validation failed";
          return yield* Effect.fail(
            new ValidationError({ message: problems, userMessage: problems }),
          );
        }
        cfg = candidate;
      }
      const nameVal = Option.getOrUndefined(opts.name) ?? cfg?.name ?? "";
      const descVal =
        Option.getOrUndefined(opts.description) ?? cfg?.description ?? "";
      const urlVal = Option.getOrUndefined(opts.url) ?? cfg?.url ?? "";
      const proxyVal =
        Option.getOrUndefined(opts.proxyUrl) ?? cfg?.proxy_url ?? "";
      const scopesVal =
        opts.scopes.length > 0
          ? opts.scopes.flatMap((s) => parseSpaceSeparated(s))
          : (cfg?.authorization_scopes ?? []);

      const input: CreateApplicationInput = {
        name: nameVal,
        description: descVal,
        url: urlVal,
        proxyUrl: proxyVal,
        authorizationScopes: scopesVal,
      };
      if (!input.name)
        return yield* Effect.fail(
          new ValidationError({
            message: "Application name is required",
            userMessage: "Application name is required",
          }),
        );
      if (!input.description)
        return yield* Effect.fail(
          new ValidationError({
            message: "Application description is required",
            userMessage: "Application description is required",
          }),
        );
      if (!input.url)
        return yield* Effect.fail(
          new ValidationError({
            message: "Application URL is required",
            userMessage: "Application URL is required",
          }),
        );
      if (!input.proxyUrl)
        return yield* Effect.fail(
          new ValidationError({
            message: "Proxy URL is required",
            userMessage: "Proxy URL is required",
          }),
        );
      if (!input.authorizationScopes.length)
        return yield* Effect.fail(
          new ValidationError({
            message: "Authorization scopes are required",
            userMessage: "Authorization scopes are required",
          }),
        );

      const environment = yield* resolveEnvironmentEffect(envStr);
      const appData = yield* applicationInitEffect(input, environment);
      yield* writer.emitSuccess(
        "godaddy application init",
        {
          id: appData.id,
          name: appData.name,
          status: appData.status,
          url: appData.url,
          proxy_url: appData.proxyUrl,
          authorization_scopes: appData.authorizationScopes,
          client_id: appData.clientId,
          files_written: {
            config: getConfigFilePath(environment),
            env: join(process.cwd(), `.env.${environment}`),
          },
        },
        appInitActions(appData.name),
      );
    }),
).pipe(Command.withDescription("Initialize/create a new application"));

// --- Add sub-tree ---

const addAction = Command.make(
  "action",
  {
    name: Options.text("name").pipe(Options.withDescription("Action name")),
    url: Options.text("url").pipe(
      Options.withDescription("Action endpoint URL"),
    ),
    config: configOption,
    environment: envOption,
  },
  (opts) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      if (opts.name.length < 3)
        return yield* Effect.fail(
          new ValidationError({
            message: "Action name must be at least 3 characters long",
            userMessage: "Action name must be at least 3 characters long",
          }),
        );
      const env = yield* resolveEnvironmentEffect(
        Option.getOrUndefined(opts.environment),
      );
      const action: ActionConfig = { name: opts.name, url: opts.url };
      yield* addActionToConfigEffect(action, {
        configPath: Option.getOrUndefined(opts.config),
        env,
      });
      yield* writer.emitSuccess(
        "godaddy application add action",
        {
          action,
          config_path: resolveConfigPath(
            Option.getOrUndefined(opts.config),
            env,
          ),
        },
        addConfigActions,
      );
    }),
).pipe(Command.withDescription("Add action configuration to godaddy.toml"));

const addSubscription = Command.make(
  "subscription",
  {
    name: Options.text("name").pipe(
      Options.withDescription("Subscription name"),
    ),
    events: Options.text("events").pipe(
      Options.withDescription("Comma-separated list of events"),
    ),
    url: Options.text("url").pipe(
      Options.withDescription("Webhook endpoint URL"),
    ),
    config: configOption,
    environment: envOption,
  },
  (opts) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      if (opts.name.length < 3)
        return yield* Effect.fail(
          new ValidationError({
            message: "Subscription name must be at least 3 characters long",
            userMessage: "Subscription name must be at least 3 characters long",
          }),
        );
      const eventList = parseCommaSeparated(opts.events);
      if (!eventList.length)
        return yield* Effect.fail(
          new ValidationError({
            message: "At least one event is required",
            userMessage: "At least one event is required",
          }),
        );
      const env = yield* resolveEnvironmentEffect(
        Option.getOrUndefined(opts.environment),
      );
      const subscription: SubscriptionConfig = {
        name: opts.name,
        events: eventList,
        url: opts.url,
      };
      yield* addSubscriptionToConfigEffect(subscription, {
        configPath: Option.getOrUndefined(opts.config),
        env,
      });
      yield* writer.emitSuccess(
        "godaddy application add subscription",
        {
          subscription,
          config_path: resolveConfigPath(
            Option.getOrUndefined(opts.config),
            env,
          ),
        },
        addConfigActions,
      );
    }),
).pipe(
  Command.withDescription(
    "Add webhook subscription configuration to godaddy.toml",
  ),
);

// Extension sub-tree

const extEmbed = Command.make(
  "embed",
  {
    name: Options.text("name").pipe(Options.withDescription("Extension name")),
    handle: Options.text("handle").pipe(
      Options.withDescription("Extension handle"),
    ),
    source: Options.text("source").pipe(
      Options.withDescription("Path to extension source file"),
    ),
    target: Options.text("target").pipe(
      Options.withDescription("Comma-separated list of target locations"),
    ),
    config: configOption,
    environment: envOption,
  },
  (opts) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      if (opts.name.length < 3)
        return yield* Effect.fail(
          new ValidationError({
            message: "Extension name must be at least 3 characters long",
            userMessage: "Extension name must be at least 3 characters long",
          }),
        );
      if (opts.handle.length < 3)
        return yield* Effect.fail(
          new ValidationError({
            message: "Extension handle must be at least 3 characters long",
            userMessage: "Extension handle must be at least 3 characters long",
          }),
        );
      const targets = parseCommaSeparated(opts.target).map((t) => ({
        target: t,
      }));
      if (!targets.length)
        return yield* Effect.fail(
          new ValidationError({
            message: "At least one valid target is required",
            userMessage: "At least one valid target is required",
          }),
        );
      const extension: EmbedExtensionConfig = {
        name: opts.name,
        handle: opts.handle,
        source: opts.source,
        targets,
      };
      const env = yield* resolveEnvironmentEffect(
        Option.getOrUndefined(opts.environment),
      );
      yield* addExtensionToConfigEffect("embed", extension, {
        configPath: Option.getOrUndefined(opts.config),
        env,
      });
      yield* writer.emitSuccess(
        "godaddy application add extension embed",
        {
          extension_type: "embed",
          extension,
          config_path: resolveConfigPath(
            Option.getOrUndefined(opts.config),
            env,
          ),
        },
        addConfigActions,
      );
    }),
).pipe(Command.withDescription("Add an embed extension"));

const extCheckout = Command.make(
  "checkout",
  {
    name: Options.text("name").pipe(Options.withDescription("Extension name")),
    handle: Options.text("handle").pipe(
      Options.withDescription("Extension handle"),
    ),
    source: Options.text("source").pipe(
      Options.withDescription("Path to extension source file"),
    ),
    target: Options.text("target").pipe(
      Options.withDescription(
        "Comma-separated list of checkout target locations",
      ),
    ),
    config: configOption,
    environment: envOption,
  },
  (opts) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      if (opts.name.length < 3)
        return yield* Effect.fail(
          new ValidationError({
            message: "Extension name must be at least 3 characters long",
            userMessage: "Extension name must be at least 3 characters long",
          }),
        );
      if (opts.handle.length < 3)
        return yield* Effect.fail(
          new ValidationError({
            message: "Extension handle must be at least 3 characters long",
            userMessage: "Extension handle must be at least 3 characters long",
          }),
        );
      const targets = parseCommaSeparated(opts.target).map((t) => ({
        target: t,
      }));
      if (!targets.length)
        return yield* Effect.fail(
          new ValidationError({
            message: "At least one valid target is required",
            userMessage: "At least one valid target is required",
          }),
        );
      const extension: CheckoutExtensionConfig = {
        name: opts.name,
        handle: opts.handle,
        source: opts.source,
        targets,
      };
      const env = yield* resolveEnvironmentEffect(
        Option.getOrUndefined(opts.environment),
      );
      yield* addExtensionToConfigEffect("checkout", extension, {
        configPath: Option.getOrUndefined(opts.config),
        env,
      });
      yield* writer.emitSuccess(
        "godaddy application add extension checkout",
        {
          extension_type: "checkout",
          extension,
          config_path: resolveConfigPath(
            Option.getOrUndefined(opts.config),
            env,
          ),
        },
        addConfigActions,
      );
    }),
).pipe(Command.withDescription("Add a checkout extension"));

const extBlocks = Command.make(
  "blocks",
  {
    source: Options.text("source").pipe(
      Options.withDescription("Path to blocks extension source file"),
    ),
    config: configOption,
    environment: envOption,
  },
  (opts) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const extension: BlocksExtensionConfig = { source: opts.source };
      const env = yield* resolveEnvironmentEffect(
        Option.getOrUndefined(opts.environment),
      );
      yield* addExtensionToConfigEffect("blocks", extension, {
        configPath: Option.getOrUndefined(opts.config),
        env,
      });
      yield* writer.emitSuccess(
        "godaddy application add extension blocks",
        {
          extension_type: "blocks",
          extension,
          config_path: resolveConfigPath(
            Option.getOrUndefined(opts.config),
            env,
          ),
        },
        addConfigActions,
      );
    }),
).pipe(Command.withDescription("Set the blocks extension source"));

const extensionGroup = Command.make("extension", {}, () =>
  Effect.gen(function* () {
    const writer = yield* EnvelopeWriter;
    yield* writer.emitSuccess(
      "godaddy application add extension",
      {
        command: "godaddy application add extension",
        description: "Add UI extension configuration to godaddy.toml",
        commands: [
          {
            command: "godaddy application add extension embed",
            description: "Add an embed extension",
          },
          {
            command: "godaddy application add extension checkout",
            description: "Add a checkout extension",
          },
          {
            command: "godaddy application add extension blocks",
            description: "Set the blocks extension source",
          },
        ],
      },
      addExtGroupActions,
    );
  }),
).pipe(
  Command.withDescription("Add UI extension configuration to godaddy.toml"),
  Command.withSubcommands([extEmbed, extCheckout, extBlocks]),
);

const addGroup = Command.make("add", {}, () =>
  Effect.gen(function* () {
    const writer = yield* EnvelopeWriter;
    yield* writer.emitSuccess(
      "godaddy application add",
      {
        command: "godaddy application add",
        description: "Add configurations to application",
        commands: [
          {
            command: "godaddy application add action",
            description: "Add action configuration to godaddy.toml",
          },
          {
            command: "godaddy application add subscription",
            description:
              "Add webhook subscription configuration to godaddy.toml",
          },
          {
            command: "godaddy application add extension",
            description: "Add UI extension configuration to godaddy.toml",
          },
        ],
      },
      addGroupActions,
    );
  }),
).pipe(
  Command.withDescription("Add configurations to application"),
  Command.withSubcommands([addAction, addSubscription, extensionGroup]),
);

const appRelease = Command.make(
  "release",
  {
    name: Args.text({ name: "name" }).pipe(
      Args.withDescription("Application name"),
    ),
    releaseVersion: Options.text("release-version").pipe(
      Options.withDescription("Release version"),
    ),
    description: Options.text("description").pipe(
      Options.withDescription("Release description"),
      Options.optional,
    ),
    config: configOption,
    environment: envOption,
  },
  (opts) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const env = yield* resolveEnvironmentEffect(
        Option.getOrUndefined(opts.environment),
      );
      const releaseInfo = yield* applicationReleaseEffect({
        applicationName: opts.name,
        version: opts.releaseVersion,
        description: Option.getOrUndefined(opts.description),
        configPath: Option.getOrUndefined(opts.config),
        env,
      });
      yield* writer.emitSuccess(
        "godaddy application release",
        {
          id: releaseInfo.id,
          version: releaseInfo.version,
          description: releaseInfo.description,
          created_at: releaseInfo.createdAt,
        },
        appReleaseActions(opts.name),
      );
    }),
).pipe(Command.withDescription("Create a new release for the application"));

const appDeploy = Command.make(
  "deploy",
  {
    name: Args.text({ name: "name" }).pipe(
      Args.withDescription("Application name"),
    ),
    config: configOption,
    environment: envOption,
    follow: Options.boolean("follow").pipe(
      Options.withDescription("Stream deploy progress as NDJSON events"),
    ),
  },
  (opts) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const follow = opts.follow;
      const cmdStr = follow
        ? "godaddy application deploy --follow"
        : "godaddy application deploy";
      const nextActions = appDeployActions(opts.name);

      if (follow) {
        yield* writer.emitStreamEvent({
          type: "start",
          command: cmdStr,
          ts: new Date().toISOString(),
        });
      }

      const env = yield* resolveEnvironmentEffect(
        Option.getOrUndefined(opts.environment),
      );

      // Stream callback for progress events — routes through EnvelopeWriter
      const onProgress = follow
        ? (event: DeployProgressEvent): void => {
            // Best-effort: use Effect.runSync to emit through the writer.
            // Stream events are non-fatal — catch and discard errors.
            try {
              if (event.type === "step" && event.status) {
                Effect.runSync(
                  writer.emitStreamEvent({
                    type: "step",
                    name: event.name,
                    status: event.status,
                    message: event.message,
                    extension_name: event.extensionName,
                    details: event.details,
                    ts: new Date().toISOString(),
                  }),
                );
              } else if (event.type === "progress") {
                Effect.runSync(
                  writer.emitStreamEvent({
                    type: "progress",
                    name: event.name,
                    percent: event.percent,
                    message: event.message,
                    details: event.details,
                    ts: new Date().toISOString(),
                  }),
                );
              }
            } catch {
              // Stream events are best-effort — never block deployment.
            }
          }
        : undefined;

      const deployResult: DeployResult = yield* applicationDeployEffect(
        opts.name,
        {
          configPath: Option.getOrUndefined(opts.config),
          env,
          onProgress,
        },
      );

      const payload = buildDeployPayload(opts.name, deployResult);

      if (follow) {
        yield* writer.emitStreamResult(cmdStr, payload, nextActions);
      } else {
        yield* writer.emitSuccess(cmdStr, payload, nextActions);
      }
    }),
).pipe(Command.withDescription("Deploy application (change status to ACTIVE)"));

// ---------------------------------------------------------------------------
// Parent command
// ---------------------------------------------------------------------------

const applicationParent = Command.make("application", {}, () =>
  Effect.gen(function* () {
    const writer = yield* EnvelopeWriter;
    yield* writer.emitSuccess(
      "godaddy application",
      {
        command: "godaddy application",
        description: "Manage applications",
        commands: [
          {
            command: "godaddy application info <name>",
            description: "Show application information",
          },
          {
            command: "godaddy application list",
            description: "List all applications",
          },
          {
            command: "godaddy application validate <name>",
            description: "Validate application configuration",
          },
          {
            command: "godaddy application update <name>",
            description: "Update application configuration",
          },
          {
            command: "godaddy application enable <name> --store-id <storeId>",
            description: "Enable application on a store",
          },
          {
            command: "godaddy application disable <name> --store-id <storeId>",
            description: "Disable application on a store",
          },
          {
            command: "godaddy application archive <name>",
            description: "Archive application",
          },
          {
            command: "godaddy application init",
            description: "Initialize/create a new application",
          },
          {
            command: "godaddy application add",
            description: "Add configurations to application",
          },
          {
            command:
              "godaddy application release <name> --release-version <version>",
            description: "Create a new release",
          },
          {
            command: "godaddy application deploy <name> [--follow]",
            description: "Deploy application",
          },
        ],
      },
      appGroupActions,
    );
  }),
).pipe(
  Command.withDescription("Manage applications"),
  Command.withSubcommands([
    appInfo,
    appList,
    appValidate,
    appUpdate,
    appEnable,
    appDisable,
    appArchive,
    appInit,
    addGroup,
    appRelease,
    appDeploy,
  ]),
);

export const applicationCommand = applicationParent;
