#!/usr/bin/env node

import * as Command from "@effect/cli/Command";
import * as Options from "@effect/cli/Options";
import * as NodeContext from "@effect/platform-node/NodeContext";
import * as Console from "effect/Console";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Logger from "effect/Logger";

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
  "GoDaddy Developer Platform CLI - Agent-first interface for platform operations";

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
// before handing it to the framework. Verbosity flags are converted to
// --log-level=X so the Effect runtime logger respects the level too.
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
  const norm = Math.min(verbosity, 3);
  if (norm >= 3) return ["--log-level", "trace", ...retained];
  if (norm === 2) return ["--log-level", "debug", ...retained];
  if (norm === 1) return ["--log-level", "info", ...retained];
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
  "-p",
  "-j",
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
// Console override — strips boilerplate lines from Effect's help renderer
// and collapses orphaned blank lines that remain.
// ---------------------------------------------------------------------------

// Effect's CLI renderer injects per-option metadata lines that add no value
// for users: type description ("A true or false value."), optionality status
// ("This setting is optional."), and enum listing ("One of the following: …").
// Strip those lines, then collapse any runs of 3+ blank lines that result.
const HELP_NOISE_RE =
  /^\s*(A true or false value\.|A user-defined piece of text\.|This setting is optional\.|This setting is required\.|One of the following:)/;

function cleanHelpText(text: string): string {
  return (
    text
      .split("\n")
      .filter((line) => !HELP_NOISE_RE.test(line))
      .join("\n")
      // Collapse runs of 3+ blank lines that appear where noisy lines were removed.
      .replace(/\n{3,}/g, "\n\n")
      // Remove blank lines immediately before indented content (option descriptions,
      // command table rows) so each entry is a tight two-line block. Blank lines
      // between top-level entries (option names, section headers) are unaffected
      // because they are not followed by whitespace.
      .replace(/\n\n(?=\s)/g, "\n")
  );
}

function makeCleanConsole(): Console.Console {
  const c = globalThis.console;
  const s = Effect.sync;
  return {
    [Console.TypeId]: Console.TypeId as Console.TypeId,
    log: (...args: ReadonlyArray<unknown>) =>
      s(() => {
        const text = args.map(String).join(" ");
        process.stdout.write(`${cleanHelpText(text)}\n`);
      }),
    error: (...args: ReadonlyArray<unknown>) =>
      s(() => c.error(...(args as unknown[]))),
    warn: (...args: ReadonlyArray<unknown>) =>
      s(() => c.warn(...(args as unknown[]))),
    info: (...args: ReadonlyArray<unknown>) =>
      s(() => c.info(...(args as unknown[]))),
    debug: (...args: ReadonlyArray<unknown>) =>
      s(() => c.debug(...(args as unknown[]))),
    trace: (...args: ReadonlyArray<unknown>) =>
      s(() => c.trace(...(args as unknown[]))),
    assert: (condition: boolean, ...args: ReadonlyArray<unknown>) =>
      s(() => c.assert(condition, ...(args as unknown[]))),
    clear: s(() => c.clear()),
    count: (label?: string) => s(() => c.count(label)),
    countReset: (label?: string) => s(() => c.countReset(label)),
    dir: (item: unknown, options?: unknown) => s(() => c.dir(item, options)),
    dirxml: (...args: ReadonlyArray<unknown>) =>
      s(() => c.dirxml(...(args as unknown[]))),
    group: (options?: { label?: string; collapsed?: boolean }) =>
      options?.collapsed
        ? s(() => c.groupCollapsed(options.label))
        : s(() => c.group(options?.label)),
    groupEnd: s(() => c.groupEnd()),
    table: (tabularData: unknown, properties?: ReadonlyArray<string>) =>
      s(() => c.table(tabularData, properties)),
    time: (label?: string) => s(() => c.time(label)),
    timeEnd: (label?: string) => s(() => c.timeEnd(label)),
    timeLog: (label?: string, ...args: ReadonlyArray<unknown>) =>
      s(() => c.timeLog(label, ...(args as unknown[]))),
    unsafe: c,
  };
}

// ---------------------------------------------------------------------------
// Effect logger — routes Effect.log* output to stderr with format awareness
// ---------------------------------------------------------------------------

function makeEffectLogger(outputFormat: OutputFormat) {
  return Logger.make<unknown, void>(({ logLevel, message, date }) => {
    const msg = typeof message === "string" ? message : String(message);
    if (outputFormat === "json") {
      process.stderr.write(
        `${JSON.stringify({ ts: date.toISOString(), level: logLevel.label.toLowerCase(), msg })}\n`,
      );
    } else {
      const time = date.toISOString().substring(11, 23);
      process.stderr.write(`[${time}] ${logLevel.label.padEnd(5)}  ${msg}\n`);
    }
  });
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
    output: Options.text("output").pipe(
      Options.withDescription(
        "Output format: json (default) or plaintext. Use -p for plaintext shorthand.",
      ),
      Options.optional,
    ),
    json: Options.boolean("json").pipe(
      Options.withAlias("j"),
      Options.withDescription("Shorthand for --output=json"),
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
  // Normalize -vv, --info, --debug before the framework sees them.
  // They become --log-level=X so Effect's runtime logger also respects the level.
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
    if (token.startsWith("--log-level=")) {
      const level = token.slice("--log-level=".length);
      if (level === "info") verbosity = Math.max(verbosity, 1);
      if (level === "debug") verbosity = Math.max(verbosity, 2);
      if (level === "trace") verbosity = Math.max(verbosity, 3);
    }
    if (token === "--log-level" && i + 1 < normalized.length) {
      const level = normalized[i + 1];
      if (level === "info") verbosity = Math.max(verbosity, 1);
      if (level === "debug") verbosity = Math.max(verbosity, 2);
      if (level === "trace") verbosity = Math.max(verbosity, 3);
    }
    if ((token === "--env" || token === "-e") && i + 1 < normalized.length) {
      envOverride = validateEnvironment(normalized[i + 1]);
      stripIndices.add(i);
      stripIndices.add(i + 1);
      i++;
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
      i++;
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

  // Replace trailing positional 'help' with '--help' so Effect's built-in
  // help handler fires. Only replaces the last token to avoid false positives
  // on flag values (e.g. --label help).
  const finalArgs = rewrittenFrameworkArgs.map((arg, i, arr) =>
    arg === "help" && i === arr.length - 1 ? "--help" : arg,
  );

  if (verbosity > 0) {
    setVerbosityLevel(verbosity);
  }

  const cliConfigLayer = makeCliConfigLayer({
    prettyPrint,
    verbosity,
    environmentOverride: envOverride,
    outputFormat,
  });

  const envelopeWriterLayer = EnvelopeWriterLive;

  const fullLayer = Layer.mergeAll(
    NodeContext.layer,
    NodeLiveLayer,
    cliConfigLayer,
    Console.setConsole(makeCleanConsole()),
    Logger.replace(Logger.defaultLogger, makeEffectLogger(outputFormat)),
  ).pipe((base) =>
    Layer.merge(base, Layer.provide(envelopeWriterLayer, cliConfigLayer)),
  );

  const program = cliRunner(["node", "godaddy", ...finalArgs]).pipe(
    Effect.catchAll((error) =>
      Effect.gen(function* () {
        const writer = yield* EnvelopeWriter;

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
  process.stderr.write(`Fatal: ${error}\n`);
  process.exitCode = 1;
});
