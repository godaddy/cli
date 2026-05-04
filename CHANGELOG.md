# @godaddy/cli

## 0.5.0

### Minor Changes

- ebac8a1: Outgoing HTTP requests now carry a consistent `User-Agent` (`godaddy-cli/<version>` from the package manifest) and a per-request `X-Request-ID` (UUID v7) on REST catalog calls, GraphQL requests, OAuth token exchange, and webhook event-type discovery. Presigned S3 artifact uploads are unchanged: only the headers returned with the presigned URL are sent, so uploads remain signature-compatible.

## 0.4.0

### Minor Changes

- d993164: Improve error classification for application lifecycle commands (`create`, `update`, `archive`, deploy upload). Server-side failures previously collapsed into a generic `NETWORK_ERROR` envelope now surface with an accurate top-level `code` (`VALIDATION_ERROR`, `FORBIDDEN`, `NOT_FOUND`, `CONFLICT`, `RATE_LIMIT_EXCEEDED`, `AUTH_REQUIRED`) and a specific, actionable `fix` string. Per-field validation details are forwarded under `error.details.fields` so consumers can render or react to individual field problems. Client-side input validation (rejected by the CLI before sending) remains distinct from server-side validation errors. Unknown server error codes and transport failures still map to `NETWORK_ERROR`, but now preserve the full HTTP context (status, response body, original error code) under `error.details.response` so agents can discriminate further when needed.

### Patch Changes

- 1e93e8c: DEVX-68: Reject non-public URLs when initializing an application. URLs pointing at localhost, loopback addresses (127.0.0.1, ::1), private IP ranges (10/8, 172.16/12, 192.168/16), link-local addresses, or .local/.localhost hostnames are now rejected up front with a ValidationError that explains a publicly reachable HTTPS URL is required, instead of being accepted and later failing at delivery time with an opaque NetworkError. Public HTTPS URLs and valid hostnames continue to be accepted (behavior unchanged)

## 0.3.0

### Minor Changes

- 05de96a: Expand the built-in Commerce API catalog with additional domains and GraphQL metadata, and normalize Commerce scope tokens across generated endpoints.

  Also improves API command behavior by resolving templated catalog paths (for example, `/stores/{storeId}/...`), validating trusted absolute API hosts, and surfacing richer structured API error details for troubleshooting.

## 0.2.3

### Patch Changes

- 1de3a3a: Fix `-c` config option handling so custom config paths are applied correctly.

## 0.2.2

### Patch Changes

- e6f6ae3: Hardened CLI security in three areas without changing intended workflows:

  - Block extension deploy path traversal by validating `handle` and `source` stay within the extension workspace.
  - Quote and escape generated `.env` values to prevent newline/comment-based env injection.
  - Restrict truncation `full_output` dump permissions to owner-only (`0700` dir, `0600` files).

  Also adds regression tests covering these protections.

## 0.2.1

### Patch Changes

- Add API catalog discovery commands (`api list`, `api describe`, `api search`) and preserve backward compatibility by routing legacy `godaddy api <endpoint>` usage to `godaddy api call <endpoint>`. Also add the public `godaddy-cli` agent skill documentation.
- b3cba2f: Security hardening: bind OAuth server to 127.0.0.1, sanitize headers in debug and --include output, HTML-escape OAuth error page, harden PowerShell keychain escaping, stop forwarding raw server errors to userMessage, redact sensitive fields in debug request body, add 120s OAuth timeout.

## 0.2.0

### Minor Changes

- 936ed58: Replace keytar native addon with cross-platform OS keychain (macOS security CLI, Linux secret-tool, Windows PasswordVault). No native Node addons required.

  Fix CLI error routing: validation guard no longer misclassifies AuthenticationError and NetworkError as input validation errors.

  Fix `application list` to use Relay connection syntax (edges/node) matching the updated GraphQL schema.

  Add `--scope` option to `auth login` for requesting additional OAuth scopes beyond the defaults.

  Add `--scope` option to `api` command with automatic re-authentication on 403: decodes the JWT to detect missing scopes, triggers the browser auth flow, and retries the request.

### Patch Changes

- c35262b: Fix `application deploy` by using the correct GraphQL enum casing when requesting the latest release.
