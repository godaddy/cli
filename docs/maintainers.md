# Maintainers Guide

This document contains pointers for internal GoDaddy developers maintaining the CLI.

## New developer onboarding

If you are a new developer on this project, read the following to get started:

- [`cli-engine` concepts](https://github.com/godaddy/cli-engine/blob/main/cli-engine/docs/concepts.md) goes over the components making up GoDaddy CLIs.
- [This repo's docs](../docs/)
- [Command authoring checklist](./command-authoring.md) covers conventions for adding commands and command groups.
- [`CONTRIBUTING.md`](../CONTRIBUTING.md) gives some general tips for setting up your workspace for development.
- [Updating embedded API specs](#updating-embedded-api-specs) explains how to resolve a failing API catalog drift check.

If you are doing agentic coding and see your agent struggling with following standards correctly, contributions to [`AGENTS.md`](../AGENTS.md) are greatly appreciated.

See the [New product onboarding guide](#new-product-onboarding) if you're adding a set of commands for a new product/service.

## New product onboarding

If you are adding a new set of commands to support a GoDaddy product/service, do the following:

1. Follow the [OAuth maintenance](#oauth-maintenance) guide to ensure authorization is set up properly.
1. Follow the [Update `CODEOWNERS` guide](#updating-codeowners) so that code reviews involve the right individuals.
1. Read through the [New developer onboarding guide](#new-developer-onboarding).

## OAuth maintenance

The `gddy` CLI calls downstream APIs using:

1. OAuth tokens acquired through a three-legged OAuth flow (default)
1. Personal Identification Tokens (PATs, can be manually configured by users to be automatically translated to OAuth tokens with specific scopes)

To integrate an API with the CLI you should do the following:

1. Ensure your API supports OAuth.
1. Ensure your API is configured as a resource server in the internal authorization portal.
1. Define OAuth scopes for your API; do not be overly granular since that may require users to go through excessive numbers of consent flows.
1. Authorize these OAuth scopes in your API.
1. Contact the `#cli-dev` channel to enable your scopes with the CLI OAuth clients for each environment.
1. Declare these OAuth scopes in the CLI in [`scopes/declarations.rs`](../rust/src/scopes/declarations.rs), which also explains how. Put each scope in the `default` section (requested at login) unless the action spends money, can take a site offline, deletes a billable resource, or is raw data access; those go in `step_up` and prompt for consent when the command that needs them runs.
1. Annotate commands with required scopes to automatically enable consent step-ups.
1. Ensure the `api.godaddy.com` gateway is configured for coarse-grained authorization with the required scopes.

## Updating CODEOWNERS

The [`CODEOWNERS`](../.github/CODEOWNERS) file is auto-generated. It merges together two sets of reviewers:

- *Global* - developers that maintain cross-cutting concerns within the CLI; they ensure that CLI standards are followed, inspect Rust code, etc. The global reviewers are added to every pull request.
- *Product* - changes to product-specific commands should be reviewed for correctness with product requirements. Product reviewers are added to pull requests that change files within product-specific source code folders.

To update this file:

1. Edit [`reviewers.json`](../.github/scripts/reviewers.json) to add per-product paths, reviewer GitHub usernames, or new per-product config sections. Sort entries alphabetically so it's easy to navigate.
2. Run `bash .github/scripts/generate-codeowners.sh` to regenerate the file from that configuration (it uses Bash-only features, so plain `sh` fails on some systems).

## Updating embedded API specs

The CLI embeds a catalog of upstream API specs (browsable with `gddy api ...`) and generates some typed clients from them. The [API catalog drift](../.github/workflows/catalog-drift.yml) workflow regenerates everything weekly and on every push to `main`, and fails when the committed copy is stale. To fix a failure:

1. Get a token that can read the spec repositories in the `gdcorp-platform` org. `gh auth token` works if your account has access; CI uses a read-only GitHub App instead.
1. From `rust/`, run `GITHUB_TOKEN=<token> cargo run -p generate-api-catalog`. Which specs are pulled is configured in [`api-catalog-sources.json`](../rust/api-catalog-sources.json).
1. Review the diff. The generator rewrites these paths, which are exactly what the workflow compares:
   - [`rust/schemas/api/`](../rust/schemas/api) — the embedded catalog. `manifest.json` carries a generated-at timestamp that changes on every run; the workflow ignores it, so a timestamp-only change is not drift.
   - `rust/domains-client/openapi/domains.oas3.json` and `rust/hosting-client/openapi/hosting.oas3.json` — inputs to the generated clients.
   - `rust/platform-app-client/graphql/schema.graphql` — input to the platform app client.
1. Run the checks in [`AGENTS.md`](../AGENTS.md#verification-checklist-required-before-finishing-work). Generated clients are rebuilt from the updated specs, so an upstream breaking change shows up as a compile error or test failure here; fix the affected commands in the same PR.
1. Commit the result as `fix(api-catalog): resync spec drift with ...` and name the APIs that changed (see [#321](https://github.com/godaddy/cli/pull/321)), since the commit title is surfaced in release notes.

Notes:

- The domains v1 `agreements` spec comes from a public endpoint and is best-effort: if the fetch fails, the generator logs a warning and leaves `domains.oas3.json` untouched. Set `SKIP_DOMAINS_REFRESH` to skip that fetch deliberately, for example when working offline. Don't commit with it set unless you mean to leave that file alone.
- Operations the upstream spec marks `x-visibility: private` are intentionally dropped from the catalog, so they will not appear after a refresh.
