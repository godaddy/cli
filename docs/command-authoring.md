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

- **Encode dynamic path segments.** Caller-supplied IDs containing `/`, `?`, `#` or `%` must not change URL structure. Encode them with `encode_path_segment` when assembling URLs by hand, and cover those characters in a test. Generated (Progenitor) clients already percent-encode path parameters, so pass raw values to their setters; pre-encoding would double-encode `%`.
- **Build next actions from a command template plus structured params, never by formatting a command line.** Don't write `format!("... --query '{query}'")`: 
a quote in the value breaks the command, and shell metacharacters can inject another one. Instead, declare the command and pass each value 
(e.g. a search query or pagination cursor) as a named param; the consumer fills them in and handles quoting. Param names must match the target command's args, 
`<hyphenated-placeholder>` names in the template must match those params, and don't declare params the target command doesn't accept.
- **Mark dry runs.** Every dry-run path returns `CommandResult::with_dry_run()` so envelope and audit consumers can tell a preview from an executed mutation. Mark every command with external effects as mutating so the engine's dry-run safeguard prevents unintended calls, and have dry-run validate and read every prerequisite the real call needs, so it fails wherever the real call would rather than reporting an impossible success.
- **Keep next actions honest.** Emit them only when they are executable and appropriate to the returned state. Never include consent-bypass flags (e.g. `--agree`) in suggested commands, and re-present approval guidance after material changes so stale approval isn't reused.
- **Constrain inputs at parse time.** Restrict flags to the API's documented values in argument parsing so invalid input fails locally with clear help, not after a network request.
- **Fail with actionable validation errors.** Every user-correctable input or manifest failure gets a stable validation error with a `fix`. Never silently turn malformed configuration into an empty payload.
- **Don't mask API results.** Surface in-band API errors as errors. Preserve empty acknowledgements explicitly; don't substitute default "success" objects or cache errors as empty data.
- **Honor protocol semantics.** Handle case-insensitive headers, relative pagination links and query-bearing paths, while preserving the original request value.
- **Report ambiguity.** When several catalog entries match, return an explicit ambiguity result rather than silently picking the first.
- **Keep output consistent.** Output schemas, default-field projections and every execution mode must agree; document mode-only fields as optional, since default rendering can otherwise discard the useful result. Test the rendered, default-projected output, and state each test's real scope.
- **Don't log sensitive payloads.** Avoid the `--debug transport` logging helpers for requests or responses that may contain customer, payment or order data.
- **Map only the error you mean to map.** When polling or retrying for eventual consistency, only the exhausted expected status (e.g. 404) becomes `not_found`. 
Network errors, 429s and 5xxs keep their real error mapping.
- **Use the selected environment.** Don't add private per-service URL overrides or `--env` flags on follow-up commands.

## 3. Write for the customer

Users don't know our system names, API names or environments.

- Help text, guides and output must not leak internal terms (service names, scopes, environment plumbing, implementation jargon).
- Command descriptions are short imperatives from the user's point of view.
- Text may address AI assistants directly when they must behave differently from a human (e.g. obtaining explicit user consent before a charge). 
Put it in a clearly marked `AI assistants:` note, and keep it separate from the customer-facing prose.
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
