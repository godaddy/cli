import * as Args from "@effect/cli/Args";
import * as Command from "@effect/cli/Command";
import * as Effect from "effect/Effect";
import { ValidationError } from "../../effect/errors";
import { protectPayload, truncateList } from "../agent/truncation";
import type { NextAction } from "../agent/types";
import {
  AVAILABLE_ACTIONS,
  loadActionInterface,
} from "../schemas/actions/index";
import { EnvelopeWriter } from "../services/envelope-writer";
// ---------------------------------------------------------------------------
// Colocated next_actions
// ---------------------------------------------------------------------------

const actionsGroupActions: NextAction[] = [
  { command: "godaddy actions list", description: "List all actions" },
  {
    command: "godaddy actions describe <action>",
    description: "Describe an action contract",
    params: { action: { description: "Action name", required: true } },
  },
];

function actionsListActions(firstAction?: string): NextAction[] {
  return [
    {
      command: "godaddy actions describe <action>",
      description: "Describe an action contract",
      params: {
        action: {
          description: "Action name",
          value: firstAction ?? "location.address.verify",
          required: true,
        },
      },
    },
    {
      command: "godaddy application add action --name <name> --url <url>",
      description: "Add action configuration",
      params: { name: { required: true }, url: { required: true } },
    },
  ];
}

function actionsDescribeActions(actionName?: string): NextAction[] {
  return [
    { command: "godaddy actions list", description: "List all actions" },
    {
      command: "godaddy application add action --name <name> --url <url>",
      description: "Add action configuration",
      params: {
        name: {
          description: "Action name",
          value: actionName ?? "",
          required: true,
        },
        url: { required: true },
      },
    },
  ];
}

// ---------------------------------------------------------------------------
// Subcommands
// ---------------------------------------------------------------------------

const actionsList = Command.make("list", {}, () =>
  Effect.gen(function* () {
    const writer = yield* EnvelopeWriter;
    const truncated = truncateList(
      AVAILABLE_ACTIONS.map((name) => ({ name })),
      "actions-list",
    );

    yield* writer.emitSuccess(
      "godaddy actions list",
      {
        actions: truncated.items,
        total: truncated.metadata.total,
        shown: truncated.metadata.shown,
        truncated: truncated.metadata.truncated,
        full_output: truncated.metadata.full_output,
      },
      actionsListActions(AVAILABLE_ACTIONS[0]),
    );
  }),
).pipe(
  Command.withDescription(
    "List all available actions that an application developer can hook into",
  ),
);

const actionsDescribe = Command.make(
  "describe",
  {
    action: Args.text({ name: "action" }).pipe(
      Args.withDescription("Action name to describe"),
    ),
  },
  ({ action }) =>
    Effect.gen(function* () {
      const writer = yield* EnvelopeWriter;
      const iface = loadActionInterface(action);

      if (!iface) {
        return yield* Effect.fail(
          new ValidationError({
            message: `Action '${action}' not found`,
            userMessage: `Action '${action}' does not exist. Run: godaddy actions list`,
          }),
        );
      }

      const payload = protectPayload(
        {
          name: iface.name,
          description: iface.description,
          request_schema: iface.requestSchema,
          response_schema: iface.responseSchema,
        },
        `actions-describe-${action}`,
      );

      yield* writer.emitSuccess(
        "godaddy actions describe",
        {
          ...payload.value,
          truncated: payload.metadata?.truncated ?? false,
          total: payload.metadata?.total,
          shown: payload.metadata?.shown,
          full_output: payload.metadata?.full_output,
        },
        actionsDescribeActions(action),
      );
    }),
).pipe(
  Command.withDescription(
    "Show detailed interface information for a specific action",
  ),
);

// ---------------------------------------------------------------------------
// Parent command
// ---------------------------------------------------------------------------

const actionsParent = Command.make("actions", {}, () =>
  Effect.gen(function* () {
    const writer = yield* EnvelopeWriter;
    yield* writer.emitSuccess(
      "godaddy actions",
      {
        command: "godaddy actions",
        description: "Manage application actions",
        commands: [
          {
            command: "godaddy actions list",
            description:
              "List all available actions that an application developer can hook into",
            usage: "godaddy actions list",
          },
          {
            command: "godaddy actions describe <action>",
            description:
              "Show detailed interface information for a specific action",
            usage: "godaddy actions describe <action>",
          },
        ],
      },
      actionsGroupActions,
    );
  }),
).pipe(
  Command.withDescription("Manage application actions"),
  Command.withSubcommands([actionsList, actionsDescribe]),
);

export const actionsCommand = actionsParent;
