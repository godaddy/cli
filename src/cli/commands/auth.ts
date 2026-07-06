import readline from "node:readline";
import * as Command from "@effect/cli/Command";
import * as Options from "@effect/cli/Options";
import * as Effect from "effect/Effect";
import {
  authLoginEffect,
  authLogoutEffect,
  authStatusEffect,
} from "../../core/auth";
import { envGetEffect } from "../../core/environment";
import {
  checkOnboardingStatusEffect,
  completeOnboardingEffect,
} from "../../core/onboarding";
import type { NextAction } from "../agent/types";
import { EnvelopeWriter } from "../services/envelope-writer";

// ---------------------------------------------------------------------------
// Agreement URLs
// ---------------------------------------------------------------------------

const AGREEMENT_URLS = {
  tos: "https://developer.commerce.godaddy.com/legal/agreements/terms-of-use",
  privacy:
    "https://developer.commerce.godaddy.com/legal/agreements/privacy-policy",
  developer:
    "https://developer.commerce.godaddy.com/legal/agreements/developer-agreement",
};

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

const authLoginOnboardingActions: NextAction[] = [
  {
    command: "godaddy application init",
    description: "Create your first application",
  },
  {
    command: "godaddy auth status",
    description: "Verify current authentication status",
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
    acceptAgreements: Options.boolean("accept-agreements").pipe(
      Options.withDescription(
        "Accept GoDaddy Developer agreements non-interactively (required for CI/scripts)",
      ),
      Options.withDefault(false),
    ),
  },
  ({ scope, acceptAgreements }) =>
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
      const env = yield* envGetEffect().pipe(
        Effect.orElseSucceed(() => "ote" as const),
      );
      const environment = String(env);

      // Check onboarding status — non-fatal if the call fails
      let onboardingError: string | undefined;
      const onboardingStatus = yield* checkOnboardingStatusEffect().pipe(
        Effect.catchAll((err) => {
          onboardingError = err.message;
          return Effect.succeed(null);
        }),
      );

      // New user (PENDING) — collect agreement acceptance then complete onboarding
      if (onboardingStatus?.status === "PENDING") {
        // Non-interactive without explicit flag: refuse to accept agreements silently
        if (!process.stdin.isTTY && !acceptAgreements) {
          yield* writer.emitError(
            "godaddy auth login",
            {
              code: "AGREEMENTS_REQUIRED",
              message:
                "Agreement acceptance is required to complete onboarding. Re-run interactively or pass --accept-agreements to confirm acceptance of the GoDaddy Developer terms.",
            },
            "Run: godaddy auth login --accept-agreements",
            [
              {
                command: "godaddy auth login --accept-agreements",
                description:
                  "Accept GoDaddy Developer agreements non-interactively",
              },
            ],
          );
          return;
        }

        // TTY: show agreement prompt on stderr before recording acceptance
        if (process.stdin.isTTY) {
          yield* Effect.promise(
            () =>
              new Promise<void>((resolve) => {
                const rl = readline.createInterface({
                  input: process.stdin,
                  output: process.stderr,
                });
                const prompt = [
                  "",
                  "By continuing, you agree to the GoDaddy Developer terms:",
                  "",
                  `  Terms of Service:      ${AGREEMENT_URLS.tos}`,
                  `  Privacy Policy:        ${AGREEMENT_URLS.privacy}`,
                  `  Developer Agreement:   ${AGREEMENT_URLS.developer}`,
                  "",
                  "Press Enter to accept and continue...",
                ].join("\n");
                rl.question(prompt, () => {
                  rl.close();
                  resolve();
                });
                rl.on("error", () => {
                  rl.close();
                  resolve();
                });
              }),
          );
        }

        const onboardingResult = yield* completeOnboardingEffect().pipe(
          Effect.catchAll((err) => {
            onboardingError = err.message;
            return Effect.succeed(null);
          }),
        );

        yield* writer.emitSuccess(
          "godaddy auth login",
          {
            authenticated: loginResult.success,
            environment,
            expires_at: loginResult.expiresAt?.toISOString(),
            scopes_requested: additionalScopes,
            onboarding: onboardingResult ? "complete" : "failed",
            org_id: onboardingResult?.organizationId,
            ...(onboardingError
              ? { note: `Onboarding error: ${onboardingError}` }
              : {}),
          },
          authLoginOnboardingActions,
        );
        return;
      }

      yield* writer.emitSuccess(
        "godaddy auth login",
        {
          authenticated: loginResult.success,
          environment,
          expires_at: loginResult.expiresAt?.toISOString(),
          scopes_requested: additionalScopes,
          onboarding:
            onboardingStatus?.status === "ACTIVE" ? "complete" : undefined,
          org_id: onboardingStatus?.orgId,
          ...(onboardingStatus === null
            ? { note: `Could not verify onboarding status: ${onboardingError}` }
            : {}),
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
