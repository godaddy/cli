# Feature flags

`gddy` ships pre-release commands behind a feature flag system. This doc explains the mechanism and how it's wired up in this repo, so you can add a new flagged command or enable pre-release features for testing.

## The stage ladder

Every command, group, or module has an implicit or explicit `Stage`:

```
Experimental < Beta < Ga
```

`Stage::Ga` is the default for anything with no flag declared — nothing is gated unless it opts in.

## Declaring a flag

Call `.with_feature_flag(key, stage)` on a `Module`, `GroupSpec`, or `CommandSpec` when building it. `key` is a stable string used for policy overrides and `flags info` lookups; it doesn't need to match the command name.

For something genuinely early and unstable, use the `Experimental` stage. If it's functionally complete but you are still collecting feedback before launch, set it to `Beta`.

A flag declared on a group or module cascades to every descendant that doesn't declare its own. A descendant can override its ancestor by declaring its own flag; the nearest one wins.

## Environment-scoped overrides

Pre-GA features can be enabled on a per-environment basis. You can override the minimum stage for an environment or set the stage for a particular feature flag key. This can be done through an `~/.config/gddy/environments.toml` file:

```toml
[dev]
min_stage = "experimental"

[test.feature_overrides]
"some-flag-key" = "beta"
```

Or via env vars, without a file: `<ENV>_MIN_STAGE=experimental`, `<ENV>_FEATURE_<KEY>=beta` (e.g. `DEV_MIN_STAGE`, `TEST_FEATURE_SOME_FLAG_KEY`).

## Inspecting flags at runtime

Run the following commands to get more information about feature flagging:

```
gddy flags list             # every flagged node: path, key, stage, visible
gddy flags info <key>       # policy for one key + every node resolving to it,
                            # and whether visibility was decided by min_stage
                            # or an override
```
