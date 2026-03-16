import * as Context from "effect/Context";
import * as Layer from "effect/Layer";
import type { Environment } from "../../core/environment";

/**
 * CLI runtime configuration.
 *
 * Carries the global flags parsed from argv (--pretty, --verbose, --debug,
 * --env) so that every layer in the dependency graph can access them without
 * mutable module-level state.
 */
export type OutputFormat = "json" | "plaintext";

export interface CliConfigShape {
  readonly prettyPrint: boolean;
  readonly verbosity: number; // 0 = silent, 1 = basic, 2 = full
  readonly environmentOverride: Environment | null;
  readonly outputFormat: OutputFormat;
}

export class CliConfig extends Context.Tag("CliConfig")<
  CliConfig,
  CliConfigShape
>() {}

/** Default (no flags). */
export const defaultCliConfig: CliConfigShape = {
  prettyPrint: false,
  verbosity: 0,
  environmentOverride: null,
  outputFormat: "json",
};

/** Build a layer from parsed global flags. */
export function makeCliConfigLayer(
  config: Partial<CliConfigShape>,
): Layer.Layer<CliConfig> {
  return Layer.succeed(CliConfig, {
    ...defaultCliConfig,
    ...config,
  });
}
