import * as Args from "@effect/cli/Args";
import * as Command from "@effect/cli/Command";
import * as Effect from "effect/Effect";
import * as Option from "effect/Option";
import {
  envGetEffect,
  envInfoEffect,
  envListEffect,
  envSetEffect,
  getEnvironmentDisplay,
} from "../../core/environment";
import type { NextAction } from "../agent/types";
import { EnvelopeWriter } from "../services/envelope-writer";
// ---------------------------------------------------------------------------
// Colocated next_actions
// ---------------------------------------------------------------------------

const envGroupActions: NextAction[] = [
  { command: "godaddy env get", description: "Get active environment" },
  { command: "godaddy env list", description: "List environments" },
  {
    command: "godaddy env set <environment>",
    description: "Set active environment",
    params: {
      environment: {
        description: "Environment name",
        enum: ["ote", "prod"],
        default: "ote",
        required: true,
      },
    },
  },
];

const envGetActions: NextAction[] = [
  {
    command: "godaddy env set <environment>",
    description: "Set active environment",
    params: { environment: { enum: ["ote", "prod"], required: true } },
  },
  {
    command: "godaddy env info [environment]",
    description: "Show environment details",
    params: { environment: { enum: ["ote", "prod"], default: "ote" } },
  },
];

const envSetActions: NextAction[] = [
  { command: "godaddy env get", description: "Get active environment" },
  {
    command: "godaddy auth status",
    description: "Check auth for active environment",
  },
];

const envListActions: NextAction[] = [
  { command: "godaddy env get", description: "Get active environment" },
  {
    command: "godaddy env set <environment>",
    description: "Set active environment",
    params: {
      environment: { enum: ["ote", "prod"], default: "ote", required: true },
    },
  },
  {
    command: "godaddy env info [environment]",
    description: "Show environment details",
    params: { environment: { enum: ["ote", "prod"], default: "ote" } },
  },
];

function envInfoActions(environment?: string): NextAction[] {
  return [
    {
      command: "godaddy env set <environment>",
      description: "Set active environment",
      params: {
        environment: {
          enum: ["ote", "prod"],
          value: environment ?? "ote",
          required: true,
        },
      },
    },
    { command: "godaddy auth status", description: "Check auth status" },
  ];
}

// ---------------------------------------------------------------------------
// Subcommands
// ---------------------------------------------------------------------------

const envList = Command.make("list", {}, () =>
  Effect.gen(function* () {
    const writer = yield* EnvelopeWriter;
    const environments = yield* envListEffect();
    const activeEnvironment = environments[0];

    yield* writer.emitSuccess(
      "godaddy env list",
      {
        active_environment: activeEnvironment,
        environments: environments.map((environment) => ({
          environment,
          display: getEnvironmentDisplay(environment),
        })),
      },
      envListActions,
    );
  }),
).pipe(Command.withDescription("List all available environments"));

const envGet = Command.make("get", {}, () =>
  Effect.gen(function* () {
    const writer = yield* EnvelopeWriter;
    const environment = yield* envGetEffect();

    yield* writer.emitSuccess(
      "godaddy env get",
      { environment },
      envGetActions,
    );
  }),
).pipe(Command.withDescription("Get current active environment"));

const envSet = Command.make(
  "set",
  {
    environment: Args.text({ name: "environment" }).pipe(
      Args.withDescription("Environment to set (ote|prod)"),
    ),
  },
  ({ environment }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const previousEnvironment = yield* envGetEffect();
      yield* envSetEffect(environment);

      yield* writer.emitSuccess(
        "godaddy env set",
        {
          previous_environment: previousEnvironment,
          environment,
        },
        envSetActions,
      );
    }),
).pipe(Command.withDescription("Set active environment"));

const envInfo = Command.make(
  "info",
  { environment: Args.text({ name: "environment" }).pipe(Args.optional) },
  ({ environment: environmentOpt }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const environment = Option.getOrUndefined(environmentOpt);
      const info = yield* envInfoEffect(environment);

      yield* writer.emitSuccess(
        "godaddy env info",
        {
          environment: info.environment,
          display: info.display,
          config_file: info.configFile,
          config_summary: info.config
            ? {
                name: info.config.name,
                client_id: info.config.client_id,
                version: info.config.version,
                url: info.config.url,
                proxy_url: info.config.proxy_url,
                authorization_scopes: info.config.authorization_scopes,
              }
            : null,
        },
        envInfoActions(info.environment),
      );
    }),
).pipe(
  Command.withDescription("Show detailed information about an environment"),
);

// ---------------------------------------------------------------------------
// Parent command
// ---------------------------------------------------------------------------

const envParent = Command.make("env", {}, () =>
  Effect.gen(function* () {
    const writer = yield* EnvelopeWriter;
    yield* writer.emitSuccess(
      "godaddy env",
      {
        command: "godaddy env",
        description: "Manage GoDaddy environments (ote, prod)",
        commands: [
          {
            command: "godaddy env list",
            description: "List all available environments",
            usage: "godaddy env list",
          },
          {
            command: "godaddy env get",
            description: "Get current active environment",
            usage: "godaddy env get",
          },
          {
            command: "godaddy env set <environment>",
            description: "Set active environment",
            usage: "godaddy env set <environment>",
          },
          {
            command: "godaddy env info [environment]",
            description: "Show detailed information about an environment",
            usage: "godaddy env info [environment]",
          },
        ],
      },
      envGroupActions,
    );
  }),
).pipe(
  Command.withDescription("Manage GoDaddy environments (ote, prod)"),
  Command.withSubcommands([envList, envGet, envSet, envInfo]),
);

export const envCommand = envParent;
