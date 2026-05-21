/**
 * ANS (Agent Name Service) commands.
 *
 * Provides lifecycle management for agents registered with the GoDaddy Agent
 * Name Service: registration, verification, certificate management, search,
 * resolution, auditing, and revocation.
 *
 * Authentication: GODADDY_KEY + GODADDY_SECRET environment variables.
 *
 * NOTE (CSR gap): The `register` command requires pre-generated CSR files
 * (--server-csr-file, --identity-csr-file). Auto-generation is not implemented
 * because there is no built-in ASN.1 CSR builder in Node.js, and adding
 * @peculiar/x509 as a new dependency needs discussion. See the PR description.
 */

import { readFile } from "node:fs/promises";
import * as Args from "@effect/cli/Args";
import * as Command from "@effect/cli/Command";
import * as Options from "@effect/cli/Options";
import * as Effect from "effect/Effect";
import {
  REVOCATION_REASONS,
  type RevocationReason,
  getAgentStatusEffect,
  getBadgeEffect,
  getCsrStatusEffect,
  getEventsEffect,
  getIdentityCertsEffect,
  getServerCertsEffect,
  registerAgentEffect,
  resolveAgentEffect,
  revokeAgentEffect,
  searchAgentsEffect,
  submitIdentityCsrEffect,
  submitServerCsrEffect,
  verifyAcmeEffect,
  verifyDnsEffect,
} from "../../core/ans";
import { ConfigurationError, ValidationError } from "../../effect/errors";
import type { NextAction } from "../agent/types";
import { EnvelopeWriter } from "../services/envelope-writer";

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

function readCsrFile(
  path: string,
  label: string,
): Effect.Effect<string, ConfigurationError, never> {
  return Effect.tryPromise({
    try: () => readFile(path, "utf8"),
    catch: (e) =>
      new ConfigurationError({
        message: `Failed to read ${label} file at '${path}': ${String(e)}`,
        userMessage: `Could not read ${label} file. Check the path and file permissions.`,
      }),
  });
}

// ---------------------------------------------------------------------------
// register
// ---------------------------------------------------------------------------

const ansRegisterActions: NextAction[] = [
  {
    command: "godaddy ans status <agent-id>",
    description: "Check registration status and retrieve DNS records",
  },
  {
    command: "godaddy ans verify-acme <agent-id>",
    description:
      "Trigger ACME domain validation after adding the _acme-challenge TXT record",
  },
];

const ansRegister = Command.make(
  "register",
  {
    host: Options.text("host").pipe(
      Options.withDescription(
        "Fully-qualified domain name where the agent is hosted (e.g. agent.example.com)",
      ),
    ),
    a2aUrl: Options.text("a2a-url").pipe(
      Options.withDescription(
        "Full A2A endpoint URL (e.g. https://agent.example.com/a2a)",
      ),
    ),
    version: Options.text("version").pipe(
      Options.withDescription("Agent version for the ANS name"),
      Options.withDefault("0.1.0"),
    ),
    displayName: Options.text("display-name").pipe(
      Options.withDescription("Human-readable display name for the agent"),
      Options.optional,
    ),
    serverCsrFile: Options.text("server-csr-file").pipe(
      Options.withDescription(
        "Path to PEM-encoded server CSR file (RSA-2048 required). Generate with the Rust or Go ANS SDK CLI.",
      ),
    ),
    identityCsrFile: Options.text("identity-csr-file").pipe(
      Options.withDescription(
        "Path to PEM-encoded identity CSR file (RSA-2048 required). Generate with the Rust or Go ANS SDK CLI.",
      ),
    ),
  },
  ({ host, a2aUrl, version, displayName, serverCsrFile, identityCsrFile }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;

      const serverCsrPem = yield* readCsrFile(serverCsrFile, "server CSR");
      const identityCsrPem = yield* readCsrFile(
        identityCsrFile,
        "identity CSR",
      );

      const pending = yield* registerAgentEffect({
        agentDisplayName:
          displayName._tag === "Some" ? displayName.value : host,
        agentHost: host,
        version,
        identityCsrPem,
        serverCsrPem,
        endpoints: [
          {
            url: a2aUrl,
            protocol: "A2A",
            transports: ["STREAMABLE-HTTP"],
          },
        ],
      });

      yield* writer.emitSuccess(
        "godaddy ans register",
        {
          ...pending,
          note: "Configure the DNS records listed in dns_records, then run verify-acme.",
        },
        ansRegisterActions,
      );
    }),
).pipe(
  Command.withDescription(
    "Register an agent with the ANS. Requires pre-generated RSA-2048 CSR files.",
  ),
);

// ---------------------------------------------------------------------------
// status
// ---------------------------------------------------------------------------

const ansStatus = Command.make(
  "status",
  {
    agentId: Args.text({ name: "agent-id" }).pipe(
      Args.withDescription("ANS agent ID returned by register"),
    ),
  },
  ({ agentId }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const status = yield* getAgentStatusEffect(agentId);

      const nextActions: NextAction[] = [];
      if (status.status === "PENDING_VALIDATION") {
        nextActions.push({
          command: `godaddy ans verify-acme ${agentId}`,
          description:
            "Trigger ACME verification after adding _acme-challenge TXT record",
        });
      }
      if (status.status === "PENDING_CERTS") {
        nextActions.push({
          command: `godaddy ans verify-dns ${agentId}`,
          description:
            "Trigger DNS record verification after adding all DNS records",
        });
      }
      if (status.status !== "REVOKED") {
        nextActions.push({
          command: `godaddy ans revoke ${agentId} --reason CESSATION_OF_OPERATION`,
          description: "Revoke this agent registration",
        });
      }

      yield* writer.emitSuccess("godaddy ans status", status, nextActions);
    }),
).pipe(Command.withDescription("Get current registration status for an agent"));

// ---------------------------------------------------------------------------
// verify-acme
// ---------------------------------------------------------------------------

const ansVerifyAcme = Command.make(
  "verify-acme",
  {
    agentId: Args.text({ name: "agent-id" }).pipe(
      Args.withDescription("ANS agent ID"),
    ),
  },
  ({ agentId }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const result = yield* verifyAcmeEffect(agentId);
      yield* writer.emitSuccess("godaddy ans verify-acme", result, [
        {
          command: `godaddy ans status ${agentId}`,
          description: "Poll for status after ACME verification",
        },
      ]);
    }),
).pipe(
  Command.withDescription(
    "Trigger ACME domain validation. Add the _acme-challenge TXT record shown in status first.",
  ),
);

// ---------------------------------------------------------------------------
// verify-dns
// ---------------------------------------------------------------------------

const ansVerifyDns = Command.make(
  "verify-dns",
  {
    agentId: Args.text({ name: "agent-id" }).pipe(
      Args.withDescription("ANS agent ID"),
    ),
  },
  ({ agentId }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const result = yield* verifyDnsEffect(agentId);
      yield* writer.emitSuccess("godaddy ans verify-dns", result, [
        {
          command: `godaddy ans status ${agentId}`,
          description: "Check status after DNS verification",
        },
      ]);
    }),
).pipe(
  Command.withDescription(
    "Trigger DNS record verification after all required DNS records have been configured.",
  ),
);

// ---------------------------------------------------------------------------
// submit-server-csr
// ---------------------------------------------------------------------------

const ansSubmitServerCsr = Command.make(
  "submit-server-csr",
  {
    agentId: Args.text({ name: "agent-id" }).pipe(
      Args.withDescription("ANS agent ID"),
    ),
    csrFile: Options.text("csr-file").pipe(
      Options.withDescription("Path to PEM-encoded server CSR file"),
    ),
  },
  ({ agentId, csrFile }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const csrPem = yield* readCsrFile(csrFile, "server CSR");
      const result = yield* submitServerCsrEffect(agentId, csrPem);
      yield* writer.emitSuccess("godaddy ans submit-server-csr", result, [
        {
          command: `godaddy ans csr-status ${agentId} --csr-id ${result.csrId}`,
          description: "Poll CSR status",
        },
      ]);
    }),
).pipe(
  Command.withDescription(
    "Submit a server CSR for an already-registered agent.",
  ),
);

// ---------------------------------------------------------------------------
// submit-identity-csr
// ---------------------------------------------------------------------------

const ansSubmitIdentityCsr = Command.make(
  "submit-identity-csr",
  {
    agentId: Args.text({ name: "agent-id" }).pipe(
      Args.withDescription("ANS agent ID"),
    ),
    csrFile: Options.text("csr-file").pipe(
      Options.withDescription("Path to PEM-encoded identity CSR file"),
    ),
  },
  ({ agentId, csrFile }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const csrPem = yield* readCsrFile(csrFile, "identity CSR");
      const result = yield* submitIdentityCsrEffect(agentId, csrPem);
      yield* writer.emitSuccess("godaddy ans submit-identity-csr", result, [
        {
          command: `godaddy ans csr-status ${agentId} --csr-id ${result.csrId}`,
          description: "Poll CSR status",
        },
      ]);
    }),
).pipe(
  Command.withDescription(
    "Submit an identity CSR for an already-registered agent.",
  ),
);

// ---------------------------------------------------------------------------
// csr-status
// ---------------------------------------------------------------------------

const ansCsrStatus = Command.make(
  "csr-status",
  {
    agentId: Args.text({ name: "agent-id" }).pipe(
      Args.withDescription("ANS agent ID"),
    ),
    csrId: Options.text("csr-id").pipe(
      Options.withDescription(
        "CSR ID returned by submit-server-csr or submit-identity-csr",
      ),
    ),
  },
  ({ agentId, csrId }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const result = yield* getCsrStatusEffect(agentId, csrId);
      yield* writer.emitSuccess("godaddy ans csr-status", result, [
        {
          command: `godaddy ans status ${agentId}`,
          description: "Check overall agent registration status",
        },
      ]);
    }),
).pipe(Command.withDescription("Get the status of a pending CSR submission."));

// ---------------------------------------------------------------------------
// revoke
// ---------------------------------------------------------------------------

const ansRevoke = Command.make(
  "revoke",
  {
    agentId: Args.text({ name: "agent-id" }).pipe(
      Args.withDescription("ANS agent ID to revoke"),
    ),
    reason: Options.text("reason").pipe(
      Options.withDescription(
        `Revocation reason. One of: ${REVOCATION_REASONS.join(", ")}`,
      ),
    ),
  },
  ({ agentId, reason }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;

      if (!REVOCATION_REASONS.includes(reason as RevocationReason)) {
        return yield* Effect.fail(
          new ValidationError({
            message: `Invalid revocation reason: '${reason}'`,
            userMessage: `Revocation reason must be one of: ${REVOCATION_REASONS.join(", ")}`,
          }),
        );
      }

      yield* revokeAgentEffect(agentId, reason as RevocationReason);
      yield* writer.emitSuccess(
        "godaddy ans revoke",
        {
          agentId,
          revoked: true,
          reason,
          note: "The ANS name lock is released by a background job. Re-registration may not be available immediately.",
        },
        [
          {
            command: "godaddy ans register",
            description: "Register a new agent",
          },
        ],
      );
    }),
).pipe(
  Command.withDescription(
    "Revoke an agent registration. Valid reasons: CESSATION_OF_OPERATION, KEY_COMPROMISE, AFFILIATION_CHANGED, SUPERSEDED, EXPIRED_CERT, UNSPECIFIED.",
  ),
);

// ---------------------------------------------------------------------------
// search
// ---------------------------------------------------------------------------

const ansSearch = Command.make(
  "search",
  {
    host: Options.text("host").pipe(
      Options.withDescription("Filter by agent hostname"),
      Options.optional,
    ),
    name: Options.text("name").pipe(
      Options.withDescription("Filter by agent display name"),
      Options.optional,
    ),
    version: Options.text("version").pipe(
      Options.withDescription("Filter by agent version"),
      Options.optional,
    ),
  },
  ({ host, name, version }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const result = yield* searchAgentsEffect({
        host: host._tag === "Some" ? host.value : undefined,
        name: name._tag === "Some" ? name.value : undefined,
        version: version._tag === "Some" ? version.value : undefined,
      });
      yield* writer.emitSuccess("godaddy ans search", result, [
        {
          command: "godaddy ans resolve --host <host>",
          description: "Resolve agent endpoints by hostname",
        },
      ]);
    }),
).pipe(
  Command.withDescription(
    "Search for registered ANS agents by host, name, or version.",
  ),
);

// ---------------------------------------------------------------------------
// resolve
// ---------------------------------------------------------------------------

const ansResolve = Command.make(
  "resolve",
  {
    host: Options.text("host").pipe(
      Options.withDescription("Hostname to resolve agents for"),
    ),
    version: Options.text("version").pipe(
      Options.withDescription(
        "Version pattern (semver range) to match, e.g. ^1.0.0",
      ),
      Options.optional,
    ),
  },
  ({ host, version }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const result = yield* resolveAgentEffect(
        host,
        version._tag === "Some" ? version.value : undefined,
      );
      yield* writer.emitSuccess("godaddy ans resolve", result, [
        {
          command: "godaddy ans status <agent-id>",
          description: "Get full status for a specific agent",
        },
      ]);
    }),
).pipe(
  Command.withDescription(
    "Resolve ANS agents by hostname and optional version pattern.",
  ),
);

// ---------------------------------------------------------------------------
// events
// ---------------------------------------------------------------------------

const ansEvents = Command.make(
  "events",
  {
    limit: Options.integer("limit").pipe(
      Options.withDescription("Maximum number of events to return"),
      Options.withDefault(50),
    ),
    lastLogId: Options.text("last-log-id").pipe(
      Options.withDescription(
        "Pagination cursor: return events after this log ID",
      ),
      Options.optional,
    ),
  },
  ({ limit, lastLogId }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const result = yield* getEventsEffect({
        limit,
        lastLogId: lastLogId._tag === "Some" ? lastLogId.value : undefined,
      });
      const nextActions: NextAction[] = [];
      if (result.nextLogId) {
        nextActions.push({
          command: `godaddy ans events --last-log-id ${result.nextLogId}`,
          description: "Fetch the next page of events",
          params: { "last-log-id": { required: true } },
        });
      }
      yield* writer.emitSuccess("godaddy ans events", result, nextActions);
    }),
).pipe(
  Command.withDescription("List ANS audit events with optional pagination."),
);

// ---------------------------------------------------------------------------
// get-server-certs
// ---------------------------------------------------------------------------

const ansGetServerCerts = Command.make(
  "get-server-certs",
  {
    agentId: Args.text({ name: "agent-id" }).pipe(
      Args.withDescription("ANS agent ID"),
    ),
  },
  ({ agentId }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const result = yield* getServerCertsEffect(agentId);
      yield* writer.emitSuccess("godaddy ans get-server-certs", result, []);
    }),
).pipe(
  Command.withDescription("Retrieve issued server certificates for an agent."),
);

// ---------------------------------------------------------------------------
// get-identity-certs
// ---------------------------------------------------------------------------

const ansGetIdentityCerts = Command.make(
  "get-identity-certs",
  {
    agentId: Args.text({ name: "agent-id" }).pipe(
      Args.withDescription("ANS agent ID"),
    ),
  },
  ({ agentId }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const result = yield* getIdentityCertsEffect(agentId);
      yield* writer.emitSuccess("godaddy ans get-identity-certs", result, []);
    }),
).pipe(
  Command.withDescription(
    "Retrieve issued identity certificates for an agent.",
  ),
);

// ---------------------------------------------------------------------------
// badge
// ---------------------------------------------------------------------------

const ansBadge = Command.make(
  "badge",
  {
    agentId: Args.text({ name: "agent-id" }).pipe(
      Args.withDescription("ANS agent ID"),
    ),
  },
  ({ agentId }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const result = yield* getBadgeEffect(agentId);
      yield* writer.emitSuccess("godaddy ans badge", result, []);
    }),
).pipe(
  Command.withDescription(
    "Get the transparency log entry and Merkle proof for an agent.",
  ),
);

// ---------------------------------------------------------------------------
// Parent command
// ---------------------------------------------------------------------------

const ansParent = Command.make("ans", {}, () =>
  Effect.gen(function* () {
    const writer = yield* EnvelopeWriter;
    yield* writer.emitSuccess(
      "godaddy ans",
      {
        description:
          "Manage agent registrations with the GoDaddy Agent Name Service",
        commands: [
          "register",
          "status",
          "verify-acme",
          "verify-dns",
          "submit-server-csr",
          "submit-identity-csr",
          "csr-status",
          "revoke",
          "search",
          "resolve",
          "events",
          "get-server-certs",
          "get-identity-certs",
          "badge",
        ],
      },
      [
        {
          command: "godaddy ans register",
          description: "Register a new agent",
        },
        {
          command: "godaddy ans search",
          description: "Search for registered agents",
        },
      ],
    );
  }),
).pipe(
  Command.withDescription(
    "Manage agent registrations with the GoDaddy Agent Name Service (ANS)",
  ),
  Command.withSubcommands([
    ansRegister,
    ansStatus,
    ansVerifyAcme,
    ansVerifyDns,
    ansSubmitServerCsr,
    ansSubmitIdentityCsr,
    ansCsrStatus,
    ansRevoke,
    ansSearch,
    ansResolve,
    ansEvents,
    ansGetServerCerts,
    ansGetIdentityCerts,
    ansBadge,
  ]),
);

export const ansCommand = ansParent;
