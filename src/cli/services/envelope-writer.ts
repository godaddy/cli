import * as Context from "effect/Context";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Ref from "effect/Ref";
import type { StreamEvent } from "../agent/stream";
import type {
  AgentErrorEnvelope,
  AgentSuccessEnvelope,
  NextAction,
} from "../agent/types";
import { CliConfig } from "./cli-config";

// ---------------------------------------------------------------------------
// Plaintext renderer
// ---------------------------------------------------------------------------

function renderValue(value: unknown, indent = 0): string {
  const pad = "  ".repeat(indent);
  if (value === null || value === undefined) return "";
  if (typeof value !== "object") return String(value);
  if (Array.isArray(value)) {
    if (value.length === 0) return "(none)";
    return value
      .map((item) => {
        const rendered = renderValue(item, indent);
        return `${pad}- ${rendered.trimStart()}`;
      })
      .join("\n");
  }
  return Object.entries(value as Record<string, unknown>)
    .filter(([, v]) => v !== undefined && v !== null)
    .map(([k, v]) => {
      const label = k.replace(/_/g, " ");
      if (typeof v === "object" && v !== null) {
        const inner = renderValue(v, indent + 1);
        if (!inner) return null;
        return `${pad}${label}:\n${inner}`;
      }
      return `${pad}${label}: ${v}`;
    })
    .filter(Boolean)
    .join("\n");
}

function renderSuccessPlaintext(result: unknown): string {
  return renderValue(result);
}

function renderErrorPlaintext(
  error: { message: string; code: string },
  fix: string,
): string {
  return `Error: ${error.message}\nFix: ${fix}`;
}

function renderStreamEventPlaintext(event: StreamEvent): string {
  switch (event.type) {
    case "start":
      return `Running ${event.command}...`;
    case "step": {
      const prefix =
        event.status === "completed"
          ? "[ok]"
          : event.status === "failed"
            ? "[FAILED]"
            : "[ .. ]";
      const suffix =
        event.status === "failed" && event.message ? `: ${event.message}` : "";
      return `${prefix} ${event.name}${suffix}`;
    }
    case "progress": {
      const pct = event.percent !== undefined ? ` ${event.percent}%` : "";
      const msg = event.message ? ` ${event.message}` : "";
      return `${event.name}:${pct}${msg}`.trim();
    }
    case "result":
      return renderSuccessPlaintext(event.result);
    case "error":
      return renderErrorPlaintext(event.error, event.fix);
  }
}

// ---------------------------------------------------------------------------
// Service interface
// ---------------------------------------------------------------------------

export interface EnvelopeWriterShape {
  /**
   * Write a success envelope to stdout. Only the first envelope write is
   * honoured — subsequent calls are silently ignored.
   */
  readonly emitSuccess: <T>(
    command: string,
    result: T,
    nextActions: NextAction[],
  ) => Effect.Effect<AgentSuccessEnvelope<T>>;

  /**
   * Write an error envelope to stdout and set process.exitCode = 1.
   */
  readonly emitError: (
    command: string,
    error: { message: string; code: string },
    fix: string,
    nextActions: NextAction[],
  ) => Effect.Effect<AgentErrorEnvelope>;

  /**
   * Write a single NDJSON stream event (non-terminal).
   * Does NOT count as the "one envelope write."
   */
  readonly emitStreamEvent: (event: StreamEvent) => Effect.Effect<void>;

  /**
   * Write a terminal NDJSON result event and mark envelope as written.
   */
  readonly emitStreamResult: <T>(
    command: string,
    result: T,
    nextActions: NextAction[],
  ) => Effect.Effect<void>;

  /**
   * Write a terminal NDJSON error event and mark envelope as written.
   */
  readonly emitStreamError: (
    command: string,
    error: { message: string; code: string },
    fix: string,
    nextActions: NextAction[],
  ) => Effect.Effect<void>;

  /** Whether an envelope (or terminal stream event) has been written. */
  readonly hasWritten: Effect.Effect<boolean>;
}

export class EnvelopeWriter extends Context.Tag("EnvelopeWriter")<
  EnvelopeWriter,
  EnvelopeWriterShape
>() {}

// ---------------------------------------------------------------------------
// Live layer
// ---------------------------------------------------------------------------

export const EnvelopeWriterLive: Layer.Layer<EnvelopeWriter, never, CliConfig> =
  Layer.effect(
    EnvelopeWriter,
    Effect.gen(function* () {
      const config = yield* CliConfig;
      const written = yield* Ref.make(false);

      const plaintext = config.outputFormat === "plaintext";

      function serializeJson(value: unknown): string {
        return config.prettyPrint
          ? JSON.stringify(value, null, 2)
          : JSON.stringify(value);
      }

      function writeLine(line: string): Effect.Effect<void> {
        return Effect.sync(() => {
          process.stdout.write(`${line}\n`);
        });
      }

      const emitSuccess = <T>(
        command: string,
        result: T,
        nextActions: NextAction[],
      ): Effect.Effect<AgentSuccessEnvelope<T>> =>
        Effect.gen(function* () {
          const alreadyWritten = yield* Ref.get(written);
          const envelope: AgentSuccessEnvelope<T> = {
            ok: true,
            command,
            result,
            next_actions: nextActions,
          };
          if (!alreadyWritten) {
            yield* writeLine(
              plaintext
                ? renderSuccessPlaintext(result)
                : serializeJson(envelope),
            );
            yield* Ref.set(written, true);
          }
          return envelope;
        });

      const emitError = (
        command: string,
        error: { message: string; code: string },
        fix: string,
        nextActions: NextAction[],
      ): Effect.Effect<AgentErrorEnvelope> =>
        Effect.gen(function* () {
          const alreadyWritten = yield* Ref.get(written);
          const envelope: AgentErrorEnvelope = {
            ok: false,
            command,
            error,
            fix,
            next_actions: nextActions,
          };
          if (!alreadyWritten) {
            yield* writeLine(
              plaintext
                ? renderErrorPlaintext(error, fix)
                : serializeJson(envelope),
            );
            yield* Ref.set(written, true);
            yield* Effect.sync(() => {
              process.exitCode = 1;
            });
          }
          return envelope;
        });

      const emitStreamEvent = (event: StreamEvent): Effect.Effect<void> =>
        writeLine(
          plaintext ? renderStreamEventPlaintext(event) : serializeJson(event),
        );

      const emitStreamResult = <T>(
        command: string,
        result: T,
        nextActions: NextAction[],
      ): Effect.Effect<void> =>
        Effect.gen(function* () {
          yield* writeLine(
            plaintext
              ? renderSuccessPlaintext(result)
              : serializeJson({
                  type: "result" as const,
                  ok: true,
                  command,
                  result,
                  next_actions: nextActions,
                }),
          );
          yield* Ref.set(written, true);
        });

      const emitStreamError = (
        command: string,
        error: { message: string; code: string },
        fix: string,
        nextActions: NextAction[],
      ): Effect.Effect<void> =>
        Effect.gen(function* () {
          yield* writeLine(
            plaintext
              ? renderErrorPlaintext(error, fix)
              : serializeJson({
                  type: "error" as const,
                  ok: false,
                  command,
                  error,
                  fix,
                  next_actions: nextActions,
                }),
          );
          yield* Ref.set(written, true);
          yield* Effect.sync(() => {
            process.exitCode = 1;
          });
        });

      const hasWritten: Effect.Effect<boolean> = Ref.get(written);

      return {
        emitSuccess,
        emitError,
        emitStreamEvent,
        emitStreamResult,
        emitStreamError,
        hasWritten,
      } satisfies EnvelopeWriterShape;
    }),
  );

// ---------------------------------------------------------------------------
// Test layer — captures envelopes instead of writing to stdout
// ---------------------------------------------------------------------------

export interface CapturedEnvelope {
  readonly kind: "success" | "error" | "stream";
  readonly value: unknown;
}

export const makeTestEnvelopeWriter = (): Effect.Effect<
  {
    service: EnvelopeWriterShape;
    captured: Ref.Ref<CapturedEnvelope[]>;
  },
  never,
  never
> =>
  Effect.gen(function* () {
    const captured = yield* Ref.make<CapturedEnvelope[]>([]);
    const written = yield* Ref.make(false);

    const push = (kind: CapturedEnvelope["kind"], value: unknown) =>
      Ref.update(captured, (list) => [...list, { kind, value }]);

    const service: EnvelopeWriterShape = {
      emitSuccess: <T>(command: string, result: T, nextActions: NextAction[]) =>
        Effect.gen(function* () {
          const envelope: AgentSuccessEnvelope<T> = {
            ok: true,
            command,
            result,
            next_actions: nextActions,
          };
          yield* push("success", envelope);
          yield* Ref.set(written, true);
          return envelope;
        }),

      emitError: (
        command: string,
        error: { message: string; code: string },
        fix: string,
        nextActions: NextAction[],
      ) =>
        Effect.gen(function* () {
          const envelope: AgentErrorEnvelope = {
            ok: false,
            command,
            error,
            fix,
            next_actions: nextActions,
          };
          yield* push("error", envelope);
          yield* Ref.set(written, true);
          return envelope;
        }),

      emitStreamEvent: (event: StreamEvent) => push("stream", event),

      emitStreamResult: <T>(
        command: string,
        result: T,
        nextActions: NextAction[],
      ) =>
        Effect.gen(function* () {
          yield* push("stream", {
            type: "result",
            ok: true,
            command,
            result,
            next_actions: nextActions,
          });
          yield* Ref.set(written, true);
        }),

      emitStreamError: (
        command: string,
        error: { message: string; code: string },
        fix: string,
        nextActions: NextAction[],
      ) =>
        Effect.gen(function* () {
          yield* push("stream", {
            type: "error",
            ok: false,
            command,
            error,
            fix,
            next_actions: nextActions,
          });
          yield* Ref.set(written, true);
        }),

      hasWritten: Ref.get(written),
    };

    return { service, captured };
  });
