#!/usr/bin/env node

import * as Command from "@effect/cli/Command";
import * as Options from "@effect/cli/Options";
import * as NodeContext from "@effect/platform-node/NodeContext";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";

import packageJson from "../package.json";
import { mapRuntimeError, mapValidationError } from "./cli/agent/errors";
import type { NextAction } from "./cli/agent/types";
import {
  type OutputFormat,
  makeCliConfigLayer,
} from "./cli/services/cli-config";
import {
  EnvelopeWriter,
  EnvelopeWriterLive,
} from "./cli/services/envelope-writer";
import { authStatusEffect } from "./core/auth";
import {
  type Environment,
  envGetEffect,
  validateEnvironment,
} from "./core/environment";
import { NodeLiveLayer } from "./effect/runtime";
import { setVerbosityLevel } from "./services/logger";

import { actionsCommand } from "./cli/commands/actions";
import { apiCommand } from "./cli/commands/api";
import { applicationCommand } from "./cli/commands/application";
import { authCommand } from "./cli/commands/auth";
// Command imports
import { envCommand } from "./cli/commands/env";
import { webhookCommand } from "./cli/commands/webhook";

// ---------------------------------------------------------------------------
// Root next_actions
// ---------------------------------------------------------------------------

const rootNextActions: NextAction[] = [
  {
    command: "godaddy auth status",
    description: "Check authentication status",
  },
  { command: "godaddy env get", description: "Get current active environment" },
  { command: "godaddy application list", description: "List all applications" },
];

// ---------------------------------------------------------------------------
// Command tree — single source of truth for discovery output.
// Keep in sync with Command.withSubcommands registrations below.
// ---------------------------------------------------------------------------

const ROOT_DESCRIPTION =
  "GoDaddy Developer Platform CLI - Agent-first JSON interface for platform operations";

interface CommandNode {
  id: string;
  command: string;
  description: string;
  usage?: string;
  children?: CommandNode[];
}

const COMMAND_TREE: CommandNode = {
  id: "root",
  command: "godaddy",
  description: ROOT_DESCRIPTION,
  children: [
    {
      id: "auth.group",
      command: "godaddy auth",
      description: "Manage authentication with GoDaddy Developer Platform",
    },
    {
      id: "env.group",
      command: "godaddy env",
      description: "Manage GoDaddy environments (ote, prod)",
    },
    {
      id: "api.group",
      command: "godaddy api",
      description: "Explore and call GoDaddy API endpoints",
      children: [
        {
          id: "api.list",
          command: "godaddy api list",
          description: "List all API domains and their endpoints",
        },
        {
          id: "api.describe",
          command: "godaddy api describe <endpoint>",
          description: "Show detailed schema information for an API endpoint",
        },
        {
          id: "api.search",
          command: "godaddy api search <query>",
          description: "Search for API endpoints by keyword",
        },
        {
          id: "api.call",
          command: "godaddy api call <endpoint>",
          description: "Make an authenticated API request",
        },
      ],
    },
    {
      id: "actions.group",
      command: "godaddy actions",
      description: "Manage application actions",
    },
    {
      id: "webhook.group",
      command: "godaddy webhook",
      description: "Manage webhook integrations",
    },
    {
      id: "application.group",
      command: "godaddy application",
      description: "Manage applications",
      children: [
        {
          id: "application.info",
          command: "godaddy application info <name>",
          description: "Show application information",
        },
        {
          id: "application.list",
          command: "godaddy application list",
          description: "List all applications",
        },
        {
          id: "application.validate",
          command: "godaddy application validate <name>",
          description: "Validate application configuration",
        },
        {
          id: "application.update",
          command: "godaddy application update <name>",
          description: "Update application configuration",
        },
        {
          id: "application.enable",
          command: "godaddy application enable <name> --store-id <storeId>",
          description: "Enable application on a store",
        },
        {
          id: "application.disable",
          command: "godaddy application disable <name> --store-id <storeId>",
          description: "Disable application on a store",
        },
        {
          id: "application.archive",
          command: "godaddy application archive <name>",
          description: "Archive application",
        },
        {
          id: "application.init",
          command: "godaddy application init",
          description: "Initialize/create a new application",
        },
        {
          id: "application.add.group",
          command: "godaddy application add",
          description: "Add configurations to application",
        },
        {
          id: "application.release",
          command:
            "godaddy application release <name> --release-version <version>",
          description: "Create a new release",
        },
        {
          id: "application.deploy",
          command: "godaddy application deploy <name> [--follow]",
          usage: "godaddy application deploy <name> [--follow]",
          description: "Deploy application",
        },
      ],
    },
  ],
};

// ---------------------------------------------------------------------------
// Global option pre-processing
//
// @effect/cli doesn't support -vv style stacking, so we normalize argv
// before handing it to the framework.
// ---------------------------------------------------------------------------

function isShortVerboseCluster(token: string): boolean {
  return /^-v{2,}$/.test(token);
}

function normalizeVerbosityArgs(argv: readonly string[]): string[] {
  const retained: string[] = [];
  let verbosity = 0;
  for (const token of argv) {
    if (token === "--debug") {
      verbosity = Math.max(verbosity, 2);
      continue;
    }
    if (token === "--info" || token === "--verbose") {
      verbosity += 1;
      continue;
    }
    if (token === "-v") {
      verbosity += 1;
      continue;
    }
    if (isShortVerboseCluster(token)) {
      verbosity += token.length - 1;
      continue;
    }
    retained.push(token);
  }
  const norm = Math.min(verbosity, 2);
  if (norm >= 2) return ["--debug", ...retained];
  if (norm === 1) return ["--verbose", ...retained];
  return retained;
}

const API_SUBCOMMANDS = new Set(["list", "describe", "search", "call"]);
const ROOT_FLAG_WITH_VALUE = new Set([
  "--env",
  "-e",
  "--output",
  "--log-level",
  "--completions",
]);
const ROOT_BOOLEAN_FLAGS = new Set([
  "--pretty",
  "--verbose",
  "-v",
  "-p",
  "-j",
  "--info",
  "--debug",
  "--help",
  "-h",
  "--version",
  "--wizard",
]);

function rewriteLegacyApiEndpointArgs(argv: readonly string[]): string[] {
  const rewritten = [...argv];
  let index = 0;

  while (index < rewritten.length) {
    const token = rewritten[index];

    if (ROOT_FLAG_WITH_VALUE.has(token)) {
      index += 2;
      continue;
    }

    if (ROOT_BOOLEAN_FLAGS.has(token)) {
      index += 1;
      continue;
    }

    if (token.startsWith("-")) {
      index += 1;
      continue;
    }

    if (token !== "api") {
      return rewritten;
    }

    const maybeSubcommandOrEndpoint = rewritten[index + 1];
    if (
      !maybeSubcommandOrEndpoint ||
      maybeSubcommandOrEndpoint.startsWith("-") ||
      API_SUBCOMMANDS.has(maybeSubcommandOrEndpoint)
    ) {
      return rewritten;
    }

    rewritten.splice(index + 1, 0, "call");
    return rewritten;
  }

  return rewritten;
}

// ---------------------------------------------------------------------------
// Root command
// ---------------------------------------------------------------------------

const rootCommand = Command.make(
  "godaddy",
  {
    pretty: Options.boolean("pretty").pipe(
      Options.withDescription(
        "Pretty-print JSON envelopes with 2-space indentation",
      ),
    ),
    verbose: Options.boolean("verbose").pipe(
      Options.withAlias("v"),
      Options.withDescription(
        "Enable basic verbose output for HTTP requests and responses",
      ),
    ),
    info: Options.boolean("info").pipe(
      Options.withDescription("Enable basic verbose output (same as -v)"),
    ),
    debug: Options.boolean("debug").pipe(
      Options.withDescription("Enable full verbose output (same as -vv)"),
    ),
    env: Options.text("env").pipe(
      Options.withAlias("e"),
      Options.withDescription(
        "Set the target environment for commands (ote, prod)",
      ),
      Options.optional,
    ),
  },
  (_config) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      // Reconstruct the command string from raw argv for traceability
      const rawArgs = process.argv.slice(2);
      const commandStr =
        rawArgs.length > 0 ? `godaddy ${rawArgs.join(" ")}` : "godaddy";

      const environment = yield* envGetEffect().pipe(
        Effect.map((env) => ({ active: env })),
        Effect.catchAll((error) => Effect.succeed({ error: error.message })),
      );

      const authSnapshot = yield* authStatusEffect().pipe(
        Effect.map(
          (status) =>
            ({
              authenticated: status.authenticated,
              has_token: status.hasToken,
              token_expiry: status.tokenExpiry?.toISOString(),
              environment: status.environment,
            }) as Record<string, unknown>,
        ),
        Effect.catchAll((error) =>
          Effect.succeed({ error: error.message } as Record<string, unknown>),
        ),
      );

      yield* writer.emitSuccess(
        commandStr,
        {
          description: COMMAND_TREE.description,
          version: packageJson.version,
          environment,
          authentication: authSnapshot,
          command_tree: COMMAND_TREE,
        },
        rootNextActions,
      );
    }),
).pipe(
  Command.withDescription(ROOT_DESCRIPTION),
  Command.withSubcommands([
    envCommand,
    authCommand,
    apiCommand,
    actionsCommand,
    webhookCommand,
    applicationCommand,
  ]),
);

// ---------------------------------------------------------------------------
// Build the runner
// ---------------------------------------------------------------------------

const cliRunner = Command.run(rootCommand, {
  name: "godaddy",
  version: packageJson.version,
});

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

export function runCli(rawArgv: ReadonlyArray<string>): Promise<void> {
  // Normalize -vv, --info, --debug before the framework sees them
  const normalized = normalizeVerbosityArgs(rawArgv);

  // Pre-parse global flags to build layers BEFORE Command.run, then strip
  // them so @effect/cli doesn't reject them as unknown subcommand options.
  let prettyPrint = false;
  let verbosity = 0;
  let envOverride: Environment | null = null;

  const envVarOutput = process.env.GODADDY_CLI_OUTPUT;
  let outputFormat: OutputFormat =
    envVarOutput === "plaintext" || envVarOutput === "json"
      ? envVarOutput
      : "json";

  const stripIndices = new Set<number>();
  for (let i = 0; i < normalized.length; i++) {
    const token = normalized[i];
    if (token === "--pretty") {
      prettyPrint = true;
      stripIndices.add(i);
    }
    if (token === "--verbose" || token === "-v")
      verbosity = Math.max(verbosity, 1);
    if (token === "--debug") verbosity = 2;
    if (token === "--info") verbosity = Math.max(verbosity, 1);
    if ((token === "--env" || token === "-e") && i + 1 < normalized.length) {
      envOverride = validateEnvironment(normalized[i + 1]);
      stripIndices.add(i);
      stripIndices.add(i + 1);
      i++; // skip value
    }
    if (token === "-p") {
      outputFormat = "plaintext";
      stripIndices.add(i);
    }
    if (token === "-j") {
      outputFormat = "json";
      stripIndices.add(i);
    }
    if (token === "--output" && i + 1 < normalized.length) {
      const val = normalized[i + 1];
      if (val === "plaintext" || val === "json") {
        outputFormat = val;
      }
      stripIndices.add(i);
      stripIndices.add(i + 1);
      i++; // skip value
    }
    if (token.startsWith("--output=")) {
      const val = token.slice("--output=".length);
      if (val === "plaintext" || val === "json") {
        outputFormat = val;
      }
      stripIndices.add(i);
    }
  }

  const frameworkArgs = normalized.filter((_, i) => !stripIndices.has(i));
  const rewrittenFrameworkArgs = rewriteLegacyApiEndpointArgs(frameworkArgs);

  // Side-effects for compatibility with existing core/ code that reads globals
  if (verbosity > 0) {
    setVerbosityLevel(verbosity);
    if (verbosity === 1) process.stderr.write("(verbose output enabled)\n");
    else process.stderr.write("(verbose output enabled: full details)\n");
  }
  const cliConfigLayer = makeCliConfigLayer({
    prettyPrint,
    verbosity,
    environmentOverride: envOverride,
    outputFormat,
  });

  const envelopeWriterLayer = EnvelopeWriterLive;

  // Full layer: platform (FileSystem, Path, Terminal) + custom services + CLI services
  const fullLayer = Layer.mergeAll(
    NodeContext.layer,
    NodeLiveLayer,
    cliConfigLayer,
  ).pipe(
    // EnvelopeWriter depends on CliConfig, so provide after merging
    (base) =>
      Layer.merge(base, Layer.provide(envelopeWriterLayer, cliConfigLayer)),
  );

  const program = cliRunner(
    // Command.run expects the full process.argv (node + script + args)
    // We pass a synthetic prefix so the framework strips the first two.
    // Use frameworkArgs (global flags already stripped) so @effect/cli
    // doesn't reject them as unknown options on subcommands.
    ["node", "godaddy", ...rewrittenFrameworkArgs],
  ).pipe(
    // Centralized error boundary: catch ALL errors, emit JSON envelope
    Effect.catchAll((error) =>
      Effect.gen(function* () {
        const writer = yield* EnvelopeWriter;

        // Check if it's an @effect/cli ValidationError (not a custom CliError)
        const CLI_VALIDATION_TAGS = new Set([
          "CommandMismatch",
          "CorrectedFlag",
          "HelpRequested",
          "InvalidArgument",
          "InvalidValue",
          "MissingFlag",
          "MissingValue",
          "MissingSubcommand",
          "MultipleValuesDetected",
          "NoBuiltInMatch",
          "UnclusteredFlag",
        ]);
        const errorTag =
          typeof error === "object" &&
          error !== null &&
          "_tag" in error &&
          typeof (error as { _tag: unknown })._tag === "string"
            ? (error as { _tag: string })._tag
            : undefined;
        const isCliValidation =
          errorTag !== undefined && CLI_VALIDATION_TAGS.has(errorTag);

        let details: { message: string; code: string; fix: string };

        if (isCliValidation) {
          // biome-ignore lint/suspicious/noExplicitAny: @effect/cli ValidationError is a union
          details = mapValidationError(error as any);
        } else {
          details = mapRuntimeError(error);
        }

        const cmdStr = `godaddy ${normalized.join(" ")}`.trim();

        // If this is a streaming command (--follow), emit stream error
        const isStreaming = normalized.includes("--follow");
        if (isStreaming) {
          yield* writer.emitStreamError(
            cmdStr,
            { message: details.message, code: details.code },
            details.fix,
            rootNextActions,
          );
        } else {
          yield* writer.emitError(
            cmdStr,
            { message: details.message, code: details.code },
            details.fix,
            rootNextActions,
          );
        }
      }),
    ),
    Effect.provide(fullLayer),
  );

  return Effect.runPromise(program);
}

// ---------------------------------------------------------------------------
// Script entry point
// ---------------------------------------------------------------------------

const args = process.argv.slice(2);
runCli(args).catch((error) => {
  // Last-resort catch for truly unexpected failures
  process.stderr.write(`Fatal: ${error}\n`);
  process.exitCode = 1;
});
