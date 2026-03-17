import * as Command from "@effect/cli/Command";
import * as Effect from "effect/Effect";
import { webhookEventsEffect } from "../../core/webhooks";
import { truncateList } from "../agent/truncation";
import type { NextAction } from "../agent/types";
import { EnvelopeWriter } from "../services/envelope-writer";
// ---------------------------------------------------------------------------
// Colocated next_actions
// ---------------------------------------------------------------------------

const webhookGroupActions: NextAction[] = [
  {
    command: "godaddy webhook events",
    description: "List available webhook events",
  },
];

const webhookEventsActions: NextAction[] = [
  {
    command:
      "godaddy application add subscription --name <name> --events <events> --url <url>",
    description: "Add a webhook subscription to config",
    params: {
      name: { description: "Subscription name", required: true },
      events: { description: "Comma-separated event list", required: true },
      url: { description: "Webhook endpoint", required: true },
    },
  },
  { command: "godaddy webhook events", description: "Refresh event list" },
];

// ---------------------------------------------------------------------------
// Subcommands
// ---------------------------------------------------------------------------

const webhookEvents = Command.make("events", {}, () =>
  Effect.gen(function* () {
    const writer = yield* EnvelopeWriter;
    const events = yield* webhookEventsEffect();
    const truncated = truncateList(events, "webhook-events");

    yield* writer.emitSuccess(
      "godaddy webhook events",
      {
        events: truncated.items,
        total: truncated.metadata.total,
        shown: truncated.metadata.shown,
        truncated: truncated.metadata.truncated,
        full_output: truncated.metadata.full_output,
      },
      webhookEventsActions,
    );
  }),
).pipe(Command.withDescription("List available webhook event types"));

// ---------------------------------------------------------------------------
// Parent command
// ---------------------------------------------------------------------------

const webhookParent = Command.make("webhook", {}, () =>
  Effect.gen(function* () {
    const writer = yield* EnvelopeWriter;
    yield* writer.emitSuccess(
      "godaddy webhook",
      {
        command: "godaddy webhook",
        description: "Manage webhook integrations",
        commands: [
          {
            command: "godaddy webhook events",
            description: "List available webhook event types",
            usage: "godaddy webhook events",
          },
        ],
      },
      webhookGroupActions,
    );
  }),
).pipe(
  Command.withDescription("Manage webhook integrations"),
  Command.withSubcommands([webhookEvents]),
);

export const webhookCommand = webhookParent;
