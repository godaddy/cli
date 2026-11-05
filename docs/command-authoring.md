# Command authoring checklist

Read this before adding a command or command group. The short, enforceable version lives in [`AGENTS.md`](../AGENTS.md); this doc adds the reasoning.

## 1. Reuse before you build

Search the codebase and `cli-engine` before writing a helper. Duplication is the most common review finding.

| Need | Use | Not |
| --- | --- | --- |
| Typed API client | Progenitor-generated client from the OpenAPI spec (as existing generated clients in the workspace do) | A hand-written `reqwest` client traversing `serde_json::Value` |
| Human output (tables, lists, footers) | `cli-engine` rendering: `HumanViewDef` / `TableColumn` | Hand-formatted strings |
| Follow-up suggestions | `cli-engine` structured next actions and its standard footer | Project-local display or placeholder-substitution code |
| Shared formatting (currency, etc.) | The existing shared helper | A per-module copy |
| URL path segments (hand-built URLs) | `api::http::encode_path_segment` | String interpolation into paths |

Prefer `cli-engine` rendering whenever it can express the output. Custom rendering should be the exception, with a reason stated in the PR. If duplication is truly unavoidable, say why.

## 2. Correctness and security

- **Encode dynamic path segments.** IDs containing `/`, `?`, `#` or `%` must not change URL structure. Use `encode_path_segment` for hand-built URLs and test those characters. Generated (Progenitor) clients already encode path params, so pass raw values to them; pre-encoding double-encodes `%`.
- **Build next actions from a command template plus structured params, never a formatted command line.** A quote in a value breaks `format!("... --query '{query}'")`, and shell metacharacters can inject commands. Param names must match the target command's args and the template's `<placeholder>`s.
- **Keep next actions honest.** Emit them only when executable and appropriate to the returned state. Never include consent-bypass flags (e.g. `--agree`), and re-present approval guidance after material changes so stale approval isn't reused.
- **Make dry runs faithful.** Mark commands with external effects as mutating so the engine's dry-run safeguard applies. Dry-run validates and reads every prerequisite the real call needs, so it fails where the real call would, and returns `CommandResult::with_dry_run()` so consumers can tell a preview from an executed mutation.
- **Keep output consistent across modes.** Preview and real runs return the same fields in camelCase, and the preview says what the real run would do (e.g. items that would fail are reported separately). Output schemas and default-field projections must include every field users should see, or default rendering drops it.
- **Test what users see.** Assert on rendered output with default fields, not just the helper that builds it. A test comment must state what the test actually exercises; rendering a hand-written JSON literal doesn't prove the handler produces it.
- **Validate early, fail actionably.** Restrict flags to the API's documented values at argument parsing. Every user-correctable input or config failure gets a stable validation error with a `fix`; never turn malformed config into an empty payload.
- **Don't turn failures or "no content" into fake successes.** APIs may report failure inside a 2xx response (an error object, or error-severity `messages`), which generated clients can deserialize as a valid empty result. Return an error and never cache it, or a transient failure is served as truth until the cache expires. A 202/204 with no body stays null/none, not `Default::default()`, which prints as a real, empty resource.
- **Follow HTTP and API conventions.** Header names are case-insensitive (`idempotency-key` matches a spec header `Idempotency-Key`). Pagination links may be relative, so read the token from the query string rather than requiring an absolute URL. Strip a user-supplied query string (`/v1/items?limit=10`) when matching the catalog, but keep it on the request.
- **Don't log sensitive payloads.** Avoid the `--debug transport` helpers for requests or responses that may contain customer, payment or order data.
- **Map only the error you mean to map.** When polling for eventual consistency, only the exhausted expected status (e.g. 404) becomes `not_found`; network errors, 429s and 5xxs keep their real mapping.
- **Use the selected environment.** No private per-service URL overrides or `--env` flags on follow-up commands.

## 3. Write for the customer

Users don't know our system names, API names or environments.

- Help text, guides and output must not leak internal terms (service names, scopes, environment plumbing, implementation jargon).
- Command descriptions are short imperatives from the user's point of view.
- Text may address AI assistants directly when they must behave differently from a human (e.g. obtaining explicit user consent before a charge). Put it in a clearly marked `AI assistants:` note, and keep it separate from the customer-facing prose.
- Prefer common terms users already know. If an API resource name differs, define it once in the guide.
- Every flag gets concrete examples and discoverable values. Don't ask for things the system can infer, and don't assume users know standards by name.
- If a command needs a value produced by an earlier command (an ID, a selection), that command's output and the guide must show where to get it.
- Don't over-communicate internals (retry mechanics, generated keys, etc.) in normal help or output. When something fails, put the suggested next step in the error's `fix` text.
- Avoid raw JSON inputs (`--body`, `--file`) in the main flow. Prefer simple flags. If JSON is unavoidable, point agents at where to get the schema instead of embedding samples.

## 4. Guides (`guides/*.md`)

- Structure: introduction (what you'll learn), concepts (what the CLI exposes, including entity relationships, opaque identifiers and consistent terminology), then task-oriented command sequences with any non-obvious step explained.
- Update discovery metadata and any related proposal docs when command availability changes, so agent matching and documentation don't contradict shipped behavior.
- Use soft line breaks in prose. The renderer wraps to the terminal width, and hard breaks look wrong in narrow terminals. Hard breaks are fine inside shell examples.
- Describe what customers can do, not which APIs are being called.

## 5. Pull requests

- Make sure the PR description matches the implemented behavior.
- Reply on each review thread with the fixing commit. Leave human reviewers' threads open for them to resolve.
