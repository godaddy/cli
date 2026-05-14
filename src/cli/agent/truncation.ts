/**
 * Context-safe truncation for agent output.
 *
 * Identical semantics to the original truncation module but with NO mutable
 * globals.  Full-output dumps use node:fs directly — the truncation path is
 * a best-effort side-channel that doesn't need to go through Effect layers.
 */

import * as fs from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const MAX_LIST_ITEMS = 50;
const MAX_STRING_LENGTH = 1000;
const MAX_SERIALIZED_BYTES = 16 * 1024;

export interface TruncationMetadata {
  truncated: boolean;
  total: number;
  shown: number;
  full_output?: string;
}

export interface ListTruncationResult<T> {
  items: T[];
  metadata: TruncationMetadata;
}

export interface PayloadTruncationResult<T> {
  value: T;
  metadata?: TruncationMetadata;
}

function estimateBytes(value: unknown): number {
  return Buffer.byteLength(JSON.stringify(value), "utf8");
}

function slugify(commandId: string): string {
  return commandId.replace(/[^a-zA-Z0-9-_.]+/g, "-");
}

export function writeFullOutput(commandId: string, payload: unknown): string {
  const dir = join(tmpdir(), "godaddy-cli");
  fs.mkdirSync(dir, { recursive: true, mode: 0o700 });
  try {
    fs.chmodSync(dir, 0o700);
  } catch {
    // Best-effort on platforms that don't honor POSIX modes.
  }
  const filename = `${Date.now()}-${slugify(commandId)}.json`;
  const fullPath = join(dir, filename);
  fs.writeFileSync(fullPath, JSON.stringify(payload, null, 2), {
    encoding: "utf8",
    mode: 0o600,
  });
  try {
    fs.chmodSync(fullPath, 0o600);
  } catch {
    // Best-effort on platforms that don't honor POSIX modes.
  }
  return fullPath;
}

function truncateStrings(value: unknown): unknown {
  if (typeof value === "string") {
    if (value.length <= MAX_STRING_LENGTH) {
      return value;
    }
    return `${value.slice(0, MAX_STRING_LENGTH)}...(truncated)`;
  }

  if (Array.isArray(value)) {
    return value.map((item) => truncateStrings(item));
  }

  if (value && typeof value === "object") {
    const result: Record<string, unknown> = {};
    for (const [key, nested] of Object.entries(value)) {
      result[key] = truncateStrings(nested);
    }
    return result;
  }

  return value;
}

export function truncateList<T>(
  items: T[],
  commandId: string,
): ListTruncationResult<T> {
  const total = items.length;
  const shown = Math.min(total, MAX_LIST_ITEMS);
  const truncated = total > MAX_LIST_ITEMS;
  const sliced = truncated ? items.slice(0, MAX_LIST_ITEMS) : items;

  if (!truncated) {
    return {
      items: sliced,
      metadata: { truncated: false, total, shown },
    };
  }

  const fullOutput = writeFullOutput(commandId, items);
  return {
    items: sliced,
    metadata: { truncated: true, total, shown, full_output: fullOutput },
  };
}

export function protectPayload<T>(
  value: T,
  commandId: string,
): PayloadTruncationResult<T> {
  const totalBytes = estimateBytes(value);
  let candidate = truncateStrings(value) as T;
  let shownBytes = estimateBytes(candidate);
  let truncated = totalBytes !== shownBytes;
  let fullOutput: string | undefined;

  if (shownBytes > MAX_SERIALIZED_BYTES) {
    truncated = true;
    const limited = {
      truncated: true,
      summary:
        typeof value === "object" && value !== null
          ? "Output too large for inline payload"
          : String(value),
    };
    candidate = limited as T;
    shownBytes = estimateBytes(candidate);
  }

  if (truncated) {
    fullOutput = writeFullOutput(commandId, value);
    return {
      value: candidate,
      metadata: {
        truncated: true,
        total: totalBytes,
        shown: shownBytes,
        full_output: fullOutput,
      },
    };
  }

  return { value: candidate };
}
