# Client Resiliency in the GoDaddy CLI

How much human review this doc has had:

- Origin: Human-Written / **AI-Assisted**
- Review: Unread / Skimmed / **Read in Full**
- Accuracy: Unverified / **Believed Accurate** / Fully Verified
- Content: Untrimmed / **Trimmed**

We are building an agent-first CLI and shipping it to customers as the primary programmatic surface for the GoDaddy Developer Platform.
Every resiliency decision we make in `gddy` becomes load characteristics on our own APIs, multiplied by every customer who installs it.

This proposal argues that retry behavior belongs to the library rather than the caller, states the risk we carry today, and lays out an ordered set of changes across `gddy` and `cli-engine`.

## The risk

Client-side retries are multiplicative, invisible to the caller, and worst exactly when the system is already degraded.
GitHub's [August 17 outage](https://github.blog/news-insights/company-news/the-august-17-outage-and-the-work-ahead/) is a current example: a client-side retry loop against Copilot services amplified traffic during an already-degraded window and extended an outage to nearly eight hours.
Their stated remediation is consistent retry limits, retry budgets, and variable timeouts enforced across service-to-service calls, not left to each caller.

We have three distinct exposures.

### 1. Unbounded hangs

Three of our four HTTP client constructors set a user agent and nothing else.

- `make_http_client()` at `rust/src/application/client.rs:16`, used by `application`, `hosting`, `webhook`, `api_explorer`, and `graphql`
- `client_with_auth()` at `rust/domains-client/src/lib.rs:135`, used by every `domain` and `dns` command
- `OnboardingClient::new()` at `rust/src/onboarding/client.rs:28`

`reqwest` applies no request timeout by default.
A degraded API therefore hangs `gddy` indefinitely, and the retry logic in `cli-engine` never fires because no attempt ever completes.
Every other item in this document depends on fixing this one first.

`gddy update` is the exception and the model to copy: 2s background and 15s foreground timeouts, applied per request (`rust/src/update/mod.rs:45`).

### 2. Synchronized retry amplification

`cli-engine` retries with deterministic exponential backoff and no jitter (`transport/client.rs:19`).
When a shared dependency fails, every client that saw the failure retries at the same instant.
Layered against a service that also retries, the amplification compounds:

```mermaid
flowchart LR
    A[1 user command] -->|3 attempts| B[gddy]
    B -->|3 attempts| C[API layer]
    C -->|3 attempts| D[Downstream service]
    D --> E[27 requests<br/>arriving in 3 synchronized bursts]
```

Two aggravating factors in our current code.
`gddy` never routes product API calls through `cli-engine`'s retrying transport; every call site builds a bare `reqwest` client and uses `cli_engine::transport` only for debug logging.
And `gddy` carries a second, independent retry implementation for artifact upload (`rust/src/application/client.rs:300`), which is how retries end up stacking at two layers inside one binary.

### 3. The agent as amplifier

Our polling commands do not loop internally.
They return a `NextAction` reading "Re-check job status" and hand control back to the agent (`rust/src/hosting/nodejs/job.rs:56`, `source/status.rs:33`, `source/git.rs:123`).
`NextAction` has no interval field (`rust/src/next_action.rs`), and nothing in `AGENTS.md`, `docs/`, or the `gddy` plugin skill tells an agent how long to wait.

An LLM handed "re-check this" with no interval will poll as fast as it can loop, and we asked it to.
That pattern appears across 100 `next_action` call sites.

Our error hints compound it.
`rust/src/error.rs:38`, `:40`, and `:43` all advise "retry" as the recovery step, including for 5xx.
Agents follow those literally.

## Current state against AWS standard mode

Source: [AWS SDKs and Tools Reference Guide, Retry behavior](https://docs.aws.amazon.com/sdkref/latest/guide/feature-retry-behavior.html).
Every value in the table below is from that page, as is the long-polling precedent in item 6.
AWS is making this behavior the default across all SDKs, currently opt-in via `AWS_NEW_RETRIES_2026=true` ([announcement](https://aws.amazon.com/blogs/developer/announcing-updated-retry-behavior-for-aws-sdks-and-tools/)).

It is a good target because it is the accumulated answer to this exact problem, already proven across every AWS service and client language.

| | AWS standard | `cli-engine` 0.8.6 | `gddy` product calls |
| --- | --- | --- | --- |
| Max attempts | 3 | 3 | 1 (no retry) |
| Backoff | `random(0,1) × min(20s, base × 2^retry)` | `500ms × 2^attempt` | none |
| Jitter | Full jitter | None | n/a |
| Base delay | 50ms transient, 1000ms throttling | 500ms for both | n/a |
| Retry quota | 500 tokens, 14/transient, 5/throttle | None | None |
| Server-directed delay | Honors `x-amz-retry-after` | Ignored | Ignored |
| Timeouts | Connect, per-attempt, overall | None | None |
| Retries 429 | Yes | Yes | No |
| Retries 5xx | By error classification | Idempotent methods only | No |

`cli-engine` sits between AWS legacy and standard mode, and it already has the retry loop, backoff, and 429 classification to build on.
`gddy`'s product calls currently bypass it, which makes routing them through it the highest-leverage change on the list.

`cli-engine` uses one 500ms base delay for every retryable error.
Transient errors and throttling responses want opposite treatment: a connection reset usually clears in milliseconds, while a 429 means the service needs time to recover capacity.
AWS splits them at 50ms and 1000ms.
A single value between the two adds latency to the first case and load to the second.

The `is_idempotent` restriction at `transport/client.rs:1282` is more conservative than AWS and we should keep it.
We do not have idempotency tokens on most endpoints, so declining to retry non-idempotent 5xx is correct.

## Proposal

Ordered by dependency. Item 6 has no dependency on the others and can land in parallel.

### 1. Timeouts on every client

Connect timeout, per-attempt timeout, and an overall deadline that bounds the full retry sequence rather than each attempt.
Without the overall deadline, three attempts of a slow request stack latency without bound.

Proposed defaults: 5s connect, 30s per attempt, 90s overall.
Transfer commands (`platform app deploy`, artifact upload) get 120s per attempt and 600s overall.

Nothing else in this list functions until this lands.

### 2. Route `gddy` through `cli-engine`'s transport

Replace the three bare `reqwest` constructors with `cli_engine::transport::HttpClient`.
This is where most of the value is, because it makes every subsequent policy change a single edit in `cli-engine` rather than a sweep across the CLI.

Delete the hand-rolled upload retry at `rust/src/application/client.rs:300` as part of this rather than fixing it in place.
Its tests at `:663` and `:695` move to cover the shared policy.

### 3. Full jitter and split base delays in `cli-engine`

Adopt AWS's formula and constants directly.

- `delay = random(0,1) × min(20s, base × 2^retry)`
- `base` = 50ms for transient errors, 1000ms for throttling
- Max attempts stays at 3

Classify on status code, since we have no modeled error codes to key off: 429 and `Retry-After`-bearing 503 are throttling, other 5xx and connection failures are transient.

### 4. Retry quota in `cli-engine`

An in-process token bucket on the AWS standard constants.

- 500 token capacity, starting full
- 14 tokens per transient retry, 5 per throttling retry
- Refund the consumed cost when a retry succeeds, plus 1 token on a first-try success
- When the budget empties, return the error without retrying

This turns "retry 3 times" from an unconditional behavior into one that yields during a broad failure.
With 3 max attempts it starts draining at roughly 22% sustained transient failure, so normal operation never notices it.
It is the item aimed most directly at the failure mode in the GitHub postmortem.

### 5. Honor `Retry-After`, in-process and across invocations

When a 429 or 503 carries `Retry-After`, use the server's value clamped the way AWS clamps it: no lower than the computed backoff, no higher than computed backoff plus 5s, and no jitter applied on top since the server is expected to jitter it.

Also persist it.
Write a single "do not call `{host}` before `T`" timestamp per environment under `~/.config/gddy/`.
On the next invocation, if `now < T`, sleep or fail fast with a message stating why and for how long.

This is one timestamp rather than an accounting system.
It is server-authoritative rather than us guessing, and it fails open when the file is missing or unparseable.
It is the only control here that survives process exit, which is what makes it the answer to a user or agent looping straight through a throttle response.

Our platform APIs do not send `Retry-After` today.
This ships as a no-op against them and starts working the moment they do, so it needs no coordination to land.
`gddy update` already established the persisted-cooldown pattern with its 24h cache TTL.

### 6. Enforced minimum poll interval

Add a minimum interval to `NextAction` and have the CLI refuse to re-poll the same resource faster than that interval, sleeping rather than passing the request through.
Default to 3s, matching the existing poll cadence in `domain purchase` (`rust/src/domain/purchase.rs:347`).

The retry quota in item 4 does not cover this, and the distinction matters.
A quota governs retries, and an agent re-invoking `gddy hosting nodejs job get` in a loop is not retrying.
Each call is a fresh request that succeeds with a "pending" status, so the bucket is never debited.
First-try successes credit it, so a hot poll loop leaves the quota full while generating unbounded load.
Each invocation is also a new process, so an in-process bucket starts full regardless.

Retries are failure-driven amplification. Poll loops are success-driven request rate. They need separate controls.

AWS reached the same conclusion for [long-polling operations](https://docs.aws.amazon.com/sdkref/latest/guide/feature-retry-behavior.html#long-polling-operations) such as `SQS.ReceiveMessage`, where the SDK applies a backoff delay before returning even when it is not retrying, specifically because those operations are called in tight loops and a fast-returning SDK makes the caller's loop hot-spin.
AWS did not document a recommended interval and trust callers to honor it.
The library enforces it.
We should do the same, because our caller is an LLM and pacing is not something we can document our way out of.

## Explicitly rejected: a persistent retry budget

A cross-invocation token bucket on disk is the intuitive answer to "what if someone runs the command in a loop." AWS does not do it, and their docs are explicit that the budget "is not shared across processes or hosts."

A retry budget bounds amplification, not load.
1000 invocations is 1000 requests: linear, visible to whoever wrote the loop, and indistinguishable from 1000 legitimate users.
Per-machine state also cannot see aggregate load (500 CI runners get 500 separate files) while adding file locking and clock-skew failure modes, where a command fails because of an unrelated invocation twenty minutes earlier.

Item 5 is the version worth building, scoped to a signal the server actually sent us.

## What this does not solve

Client-side controls are advisory.
They reduce amplification and stop us from making a bad situation worse, and they cannot protect our APIs from aggregate load, because no client can see aggregate load.
Server-side throttling remains the actual defense, and this work should not be mistaken for it.
