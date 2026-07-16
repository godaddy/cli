---
name: godaddy-cli
description: "Use the GoDaddy CLI (godaddy) to manage applications, authentication, environments, deployments, extensions, and webhooks on the GoDaddy Developer Platform. Load this skill when a task involves running godaddy commands, parsing their JSON output, managing GoDaddy applications, deploying extensions, or interacting with the GoDaddy commerce APIs."
version: 1.0.0
author: GoDaddy Commerce
tags: [godaddy, cli, commerce, applications, deploy]
---

# Using the GoDaddy CLI

The `godaddy` CLI is an agent-first tool. Regular executable commands in JSON mode return a single GoDaddy JSON envelope to stdout. Help and version output remain text, non-JSON formats pass through in their native shape, and streaming commands are outside the regular envelope adapter.

## Quick Start

```bash
# Discover all commands and check current state
godaddy

# Authenticate (opens browser for OAuth)
godaddy auth login

# Check auth + environment
godaddy auth status
godaddy env get

# List applications
godaddy application list

# Get details on one application
godaddy application info <name>
```

## Output Contract

### Regular command responses are JSON

Regular commands write one JSON object to stdout followed by a newline. This includes error envelopes, which are written to stdout while retaining a non-zero exit code. Help, version, and non-JSON diagnostics retain their text format and original stream.

### Success

```json
{
  "ok": true,
  "command": "gddy env get",
  "result": {
    "apiUrl": "https://api.godaddy.com",
    "env": "prod"
  },
  "next_actions": []
}
```

### Error

```json
{
  "ok": false,
  "command": "gddy env get",
  "error": {
    "message": "unknown environment \"not-an-environment\"; known: ote, prod",
    "code": "ERROR"
  },
  "next_actions": []
}
```

Check `ok` first. On failure, read `error.code` and `error.message`; error codes are command-specific.

### next_actions (HATEOAS)

Every response includes `next_actions` — an array of commands you can run next. These are contextual: they change based on what just happened.

```json
{
  "next_actions": [
    {
      "command": "godaddy application validate <name>",
      "description": "Validate application configuration",
      "params": {
        "name": { "value": "wes-test-app-devs2", "required": true }
      }
    },
    {
      "command": "godaddy application release <name> --release-version <version>",
      "description": "Create a release",
      "params": {
        "name": { "value": "wes-test-app-devs2", "required": true },
        "version": { "required": true }
      }
    }
  ]
}
```

How to read `next_actions`:
- **No `params`**: the command is literal — run it as-is.
- **`params` present**: the command is a template. Fill `<placeholders>` with values.
- **`params.*.value`**: pre-filled from context. Use this value unless you have a reason to override.
- **`params.*.default`**: value to use if the param is omitted.
- **`params.*.enum`**: valid choices for this param.
- **`params.*.required`**: must be provided (corresponds to `<positional>` args).

Template syntax: `<required>` for positional args, `[--flag <value>]` for optional flags, `[--flag]` for optional booleans.

### Truncated output

Large results are automatically truncated to protect context windows. When this happens:

```json
{
  "result": {
    "events": [ ... ],
    "total": 190,
    "shown": 50,
    "truncated": true,
    "full_output": "/var/folders/.../godaddy-cli/1771947169904-webhook-events.json"
  }
}
```

If `truncated` is `true`, the complete data is at the `full_output` file path. Read that file if you need everything.

## Global Options

| Flag | Alias | Effect |
|------|-------|--------|
| `--env <env>` | | Override environment for this command (`ote` or `prod`) |
| `--debug` | | Enable debug logging on stderr |
| `--output <format>` | `-o` | Select `json`, `human`, or `toon` output |
| `--json` | | Shorthand for `--output json` |
| `--human` | | Shorthand for `--output human` |
| `--toon` | | Shorthand for `--output toon` |

These options can appear anywhere in the command. The GoDaddy adapter applies to regular commands only when cli-engine renders JSON. JSON envelopes use two-space indentation by default; `--pretty` is not registered.

## Discovery

Run any group command without a subcommand to get its command tree:

```bash
godaddy                    # Full tree + environment + auth snapshot
godaddy application        # Application subcommands
godaddy application add    # Add configuration subcommands
godaddy auth               # Auth subcommands
godaddy env                # Environment subcommands
godaddy actions            # Action subcommands
godaddy webhook            # Webhook subcommands
```

The root command (`godaddy` with no args) returns the complete `command_tree`, current environment, and authentication state — everything needed to decide what to do next.

## Environments

Two environments: `ote` (test, default) and `prod`.

```bash
godaddy env get                  # Check current
godaddy env set prod             # Switch to production
godaddy env list                 # List all with active first
godaddy env info ote             # Show config details for an environment
godaddy --env prod app list      # One-off override without switching
```

The active environment determines which API endpoint and config file are used.

## Authentication

OAuth 2.0 PKCE flow. Opens a browser for login. Tokens are stored in the OS keychain.

```bash
godaddy auth login                           # Standard login
godaddy auth login --scope commerce.orders:read  # Request additional scope
godaddy auth status                          # Check token state
godaddy auth logout                          # Clear credentials
```

If a command fails with `AUTH_REQUIRED`, run `godaddy auth login` and retry.

The `godaddy api` command supports automatic re-auth: if a request returns 403 and `--scope` was provided, it re-authenticates with the requested scope and retries once.

## Commands

### Application Lifecycle

```bash
# List and inspect
godaddy application list
godaddy application info <name>
godaddy application validate <name>

# Create
godaddy application init \
  --name my-app \
  --description "My application" \
  --url https://my-app.example.com \
  --proxy-url https://my-app.example.com/api \
  --scopes "apps.app-registry:read apps.app-registry:write"

# Update
godaddy application update <name> --label "New Label"
godaddy application update <name> --status INACTIVE
godaddy application update <name> --description "Updated description"

# Enable/disable on a store
godaddy application enable <name> --store-id <storeId>
godaddy application disable <name> --store-id <storeId>

# Archive
godaddy application archive <name>
```

### Configuration (godaddy.toml)

Add actions, subscriptions, and extensions to the config file:

```bash
# Actions
godaddy application add action --name my-action --url /actions/handler

# Webhook subscriptions
godaddy application add subscription \
  --name order-events \
  --events "commerce.order.created,commerce.order.updated" \
  --url /webhooks/orders

# Extensions
godaddy application add extension embed \
  --name my-widget \
  --handle my-widget-ext \
  --source src/extensions/widget/index.tsx \
  --target admin.product.detail

godaddy application add extension checkout \
  --name my-checkout \
  --handle my-checkout-ext \
  --source src/extensions/checkout/index.tsx \
  --target checkout.cart.summary

godaddy application add extension blocks --source src/extensions/blocks/index.tsx
```

All `add` commands accept `--config <path>` and `--environment <env>` to target a specific config file.

### Release and Deploy

```bash
# Create a release (required before deploy)
godaddy application release <name> --release-version 1.0.0
godaddy application release <name> --release-version 1.0.0 --description "Initial release"

# Deploy (this command streams progress)
godaddy application deploy <name>
```

Release and deploy accept `--config <path>` and `--environment <env>`.

### API Discovery and Requests

Use `godaddy api` for endpoint discovery and authenticated API calls:

```bash
# List API domains, and endpoints within a domain
godaddy api domain list
godaddy api endpoint list --domain commerce

# Describe one endpoint (operation ID or path)
godaddy api describe commerce.location.verify-address
godaddy api describe /location/addresses

# Search endpoints by keyword
godaddy api search address
godaddy api search catalog

# Call endpoint (explicit form)
godaddy api call /v1/commerce/catalog/products
godaddy api call /v1/some/endpoint -X POST -f "name=value" -f "count=5"
godaddy api call /v1/some/endpoint -X POST -F body.json
godaddy api call /v1/some/endpoint -H "X-Custom: value"
godaddy api call /v1/some/endpoint -q ".data[0].id"
godaddy api call /v1/some/endpoint -i
godaddy api call /v1/commerce/orders -s commerce.orders:read
```

Compatibility behavior:
- `godaddy api <endpoint>` still works. If the token after `api` is not one of `domain`, `endpoint`, `describe`, `search`, or `call`, the CLI treats it as an endpoint and executes `api call`.
- This means legacy usage like `godaddy api /v1/commerce/location/addresses` remains supported.

As with other large result sets, `api domain list` / `api endpoint list` may be truncated in the inline JSON response. When `truncated: true`, read the `full_output` file path for complete results.

Address task rule:
- For requests like "find/search/verify an address", start with:
  `godaddy api search address`
  `godaddy api endpoint list --domain location`
  `godaddy api describe /location/addresses`
- Do not use `godaddy actions describe` for generic API endpoint discovery.

### Actions

`godaddy actions` is only for developers discovering action hook contracts they can integrate with as providers (or consume as a contract in app configuration). It is not for API endpoint discovery or API calls.

```bash
# List all available actions
godaddy actions list

# Describe an action's request/response contract
godaddy actions describe commerce.taxes.calculate
godaddy actions describe commerce.payment.process
```

Use `godaddy actions list` to discover current action names and then `godaddy actions describe <action>` to inspect request/response schemas for that hook contract.

### Webhooks

```bash
# List all available webhook event types
godaddy webhook events
```

Returns up to 50 events inline; use `full_output` path for the complete list (190+ events).

## NDJSON Streaming

Streaming commands are not adapted by the regular-command output envelope.
Their terminal `result` and `error` event contract is tracked separately and is
not guaranteed by the current binary. Do not assume the final line has the
regular envelope shape.

## Typical Workflows

### Create and deploy a new application

```bash
godaddy env get                                          # 1. Check environment
godaddy auth status                                      # 2. Verify auth
godaddy application init --name my-app \                 # 3. Create app
  --description "My app" \
  --url https://my-app.example.com \
  --proxy-url https://my-app.example.com/api \
  --scopes "apps.app-registry:read apps.app-registry:write"
godaddy application add action --name my-action \        # 4. Add action
  --url /actions/handler
godaddy application validate my-app                      # 5. Validate
godaddy application release my-app \                     # 6. Release
  --release-version 1.0.0
godaddy application deploy my-app                        # 7. Deploy
godaddy application enable my-app --store-id <storeId>   # 8. Enable
```

### Update and redeploy

```bash
godaddy application info my-app                          # 1. Check current state
godaddy application update my-app --description "New"    # 2. Update
godaddy application validate my-app                      # 3. Validate
godaddy application release my-app \                     # 4. Bump version
  --release-version 1.1.0
godaddy application deploy my-app                        # 5. Deploy
```

### Diagnose failures

```bash
godaddy                            # 1. Check overall state
godaddy auth status                # 2. Token expired? → godaddy auth login
godaddy env info                   # 3. Config correct?
godaddy application validate <n>   # 4. Config issues?
godaddy application info <n>       # 5. App status?
```

## Parsing Tips

1. **Parse regular command stdout as JSON when using JSON mode.** Help, version, and explicitly selected non-JSON formats are not GoDaddy envelopes.
2. **Check `ok` first.** Branch on `true`/`false` before reading `result` or `error`.
3. **Use `next_actions`** to discover what to do next. Fill template params from context.
4. **Exit code**: 0 = success, non-zero = error. Check the JSON `ok` field as well.
5. **Error envelopes use stdout.** stderr is reserved for tracing and non-envelope diagnostics.
6. **Truncated lists**: check `truncated` field. Read `full_output` file for complete data.
7. **Streaming**: do not assume a terminal public envelope until that separate contract is implemented.
