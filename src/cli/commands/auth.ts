import * as Command from "@effect/cli/Command";
import * as Options from "@effect/cli/Options";
import * as Effect from "effect/Effect";
import {
  authLoginEffect,
  authLogoutEffect,
  authStatusEffect,
} from "../../core/auth";
import { envGetEffect } from "../../core/environment";
import type { NextAction } from "../agent/types";
import { EnvelopeWriter } from "../services/envelope-writer";
// ---------------------------------------------------------------------------
// Colocated next_actions
// ---------------------------------------------------------------------------

const authGroupActions: NextAction[] = [
  { command: "godaddy auth login", description: "Login" },
  { command: "godaddy auth status", description: "Check auth status" },
];

const authLoginActions: NextAction[] = [
  {
    command: "godaddy auth status",
    description: "Verify current authentication status",
  },
  {
    command: "godaddy application list",
    description: "List applications for the active account",
  },
  { command: "godaddy auth logout", description: "Logout" },
];

const authLogoutActions: NextAction[] = [
  { command: "godaddy auth login", description: "Authenticate again" },
  { command: "godaddy auth status", description: "Check auth status" },
];

function authStatusActions(authenticated: boolean): NextAction[] {
  if (!authenticated) {
    return [
      {
        command: "godaddy auth login",
        description: "Authenticate with GoDaddy",
      },
      {
        command: "godaddy env get",
        description: "Check the active environment",
      },
    ];
  }
  return [
    { command: "godaddy application list", description: "List applications" },
    { command: "godaddy env get", description: "Check active environment" },
  ];
}

// ---------------------------------------------------------------------------
// Subcommands
// ---------------------------------------------------------------------------

const authLogin = Command.make(
  "login",
  {
    scope: Options.text("scope").pipe(
      Options.withAlias("s"),
      Options.withDescription(
        "Additional OAuth scope to request (can be repeated)",
      ),
      Options.repeated,
    ),
  },
  ({ scope }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const additionalScopes =
        scope.length > 0
          ? scope.flatMap((s) =>
              s
                .split(/[\s,]+/)
                .map((t) => t.trim())
                .filter((t) => t.length > 0),
            )
          : undefined;
      const loginResult = yield* authLoginEffect({ additionalScopes });
      const environment = yield* envGetEffect().pipe(
        Effect.map(String),
        Effect.orElseSucceed(() => "unknown"),
      );

      yield* writer.emitSuccess(
        "godaddy auth login",
        {
          authenticated: loginResult.success,
          environment,
          expires_at: loginResult.expiresAt?.toISOString(),
          scopes_requested: additionalScopes,
        },
        authLoginActions,
      );
    }),
).pipe(Command.withDescription("Login to GoDaddy Developer Platform"));

const authLogout = Command.make("logout", {}, () =>
  Effect.gen(function* () {
    const writer = yield* EnvelopeWriter;
    yield* authLogoutEffect();
    const environment = yield* envGetEffect().pipe(
      Effect.map(String),
      Effect.orElseSucceed(() => "unknown"),
    );

    yield* writer.emitSuccess(
      "godaddy auth logout",
      { authenticated: false, environment },
      authLogoutActions,
    );
  }),
).pipe(Command.withDescription("Logout and clear stored credentials"));

const authStatus = Command.make("status", {}, () =>
  Effect.gen(function* () {
    const writer = yield* EnvelopeWriter;
    const status = yield* authStatusEffect();

    yield* writer.emitSuccess(
      "godaddy auth status",
      {
        authenticated: status.authenticated,
        has_token: status.hasToken,
        token_expiry: status.tokenExpiry?.toISOString(),
        environment: status.environment,
      },
      authStatusActions(status.authenticated),
    );
  }),
).pipe(Command.withDescription("Check authentication status"));

// ---------------------------------------------------------------------------
// Parent command
// ---------------------------------------------------------------------------

const authParent = Command.make("auth", {}, () =>
  Effect.gen(function* () {
    const writer = yield* EnvelopeWriter;
    yield* writer.emitSuccess(
      "godaddy auth",
      {
        command: "godaddy auth",
        description: "Manage authentication with GoDaddy Developer Platform",
        commands: [
          {
            command: "godaddy auth login",
            description: "Login to GoDaddy Developer Platform",
            usage: "godaddy auth login",
          },
          {
            command: "godaddy auth logout",
            description: "Logout and clear stored credentials",
            usage: "godaddy auth logout",
          },
          {
            command: "godaddy auth status",
            description: "Check authentication status",
            usage: "godaddy auth status",
          },
        ],
      },
      authGroupActions,
    );
  }),
).pipe(
  Command.withDescription(
    "Manage authentication with GoDaddy Developer Platform",
  ),
  Command.withSubcommands([authLogin, authLogout, authStatus]),
);

export const authCommand = authParent;
