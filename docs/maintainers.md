# Maintainers Guide

This document contains pointers for internal GoDaddy developers maintaining the CLI.

## New developer onboarding

If you are a new developer on this project, read the following to get started:

- [`cli-engine` concepts](https://github.com/godaddy/cli-engine/blob/main/cli-engine/docs/concepts.md) goes over the components making up GoDaddy CLIs.
- [This repo's docs](../docs/)
- [`CONTRIBUTING.md`](../CONTRIBUTING.md) gives some general tips for setting up your workspace for development.

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
1. Register these OAuth scopes in the CLI within [`scopes.rs`](../rust/src/scopes.rs).
1. Annotate commands with required scopes to automatically enable consent step-ups.
1. Ensure the `api.godaddy.com` gateway is configured for coarse-grained authorization with the required scopes.

## Updating CODEOWNERS

The [`CODEOWNERS`](../.github/CODEOWNERS) file is auto-generated. It merges together two sets of reviewers:

- *Global* - developers that maintain cross-cutting concerns within the CLI; they ensure that CLI standards are followed, inspect Rust code, etc. The global reviewers are added to every pull request.
- *Product* - changes to product-specific commands should be reviewed for correctness with product requirements. Product reviewers are added to pull requests that change files within product-specific source code folders.

To update this file:

1. Edit [`reviewers.json`](../.github/scripts/reviewers.json) to add per-product paths, reviewer GitHub usernames, or new per-product config sections. Sort entries alphabetically so it's easy to navigate.
2. Run `bash .github/scripts/generate-codeowners.sh` to regenerate the file from that configuration (it uses Bash-only features, so plain `sh` fails on some systems).
