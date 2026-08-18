# CLI Telemetry — Design Document

**Date:** 2026-08-13  
**Status:** Draft  
**Author:** (TBD)

---

## Problem Statement

The GoDaddy CLI (`gddy`) has **zero product telemetry** today. We have no visibility into:

- Which commands are used, how often, and in what combinations
- Error rates and failure patterns across the user base
- Command execution latency (are API calls slow? which ones?)
- Feature adoption (are new commands being discovered?)
- AI-agent vs. human usage patterns
- Environment distribution (OS, shell, CI vs. interactive)

Without this data, product decisions (what to build, what to deprecate, what's broken) are driven
by anecdotal feedback rather than real usage signal.

### Why now?

1. **AI-agent adoption** — `gddy` is increasingly invoked by MCP clients, Cursor, and other
   AI agents. Understanding agent vs. human usage patterns is critical for API design decisions.
2. **Growing command surface** — the CLI now spans domains, hosting, API explorer, extensions,
   GraphQL, and platform apps. We need signal on which areas matter.
3. **Industry norm** — Stripe CLI, GitHub CLI (`gh`), Vercel CLI, and Next.js all ship telemetry.
   As of 2026, developer CLI telemetry is expected infrastructure, not an exception.

---

## Current State

### What exists in cli-engine (the framework)

cli-engine **already has the hooks for telemetry** — they're just not wired up:

| Component | Status | Location |
|---|---|---|
| `ActivityEmitter` trait | Defined, not implemented | `src/middleware.rs` |
| `Auditor` trait | Defined, not implemented | `src/middleware.rs` |
| `ActivityEvent` struct | Fully specified | `src/middleware.rs` |
| Middleware integration | Emits on every command outcome | `Middleware::run()` step 6 |
| Config file support | Consumer-owned keys supported | `src/config.rs` |
| `tracing` crate | Internal diagnostics only | Throughout |

**`ActivityEvent` already captures:**
- `timestamp`, `app`, `command`, `env`, `backend`
- `identity`, `sub`, `account_type`
- `status` (`ok`, `error`, `denied`, `auth-error`, `dry-run`)
- `error`, `reason`, `duration_ms`
- `args` (the raw argument map)
- `meta` (reserved extension map — currently empty)

**What's NOT in cli-engine:** telemetry transport, consent management, PII scrubbing, sampling,
or batch/flush mechanisms.

### What exists in gddy (the CLI)

| Component | Status |
|---|---|
| `tracing-subscriber` | Initialized — stderr, `RUST_LOG` only |
| `.with_activity(...)` | **Not called** in `main.rs` |
| `.with_auditor(...)` | **Not called** in `main.rs` |
| Consent config | No `[telemetry]` section in config |
| Privacy/opt-out | Not implemented |
| `gddy telemetry` command | Does not exist |
| `DO_NOT_TRACK` / env var | Not checked |

### Paths that skip activity emission today

Even once wired, these paths do **not** emit `ActivityEvent`:

- `--search` bypass (pre-clap)
- `--schema` bypass at CLI level
- Built-ins: `help`, `tree`, `guide`, `completion`, bare root help
- Clap parse errors, unknown commands
- `argv0` dispatch errors

Only **registered leaf commands** (including streaming commands) pass through
`Middleware::run()` where audit/activity hooks fire.

---

## Industry Survey

### How comparable CLIs handle telemetry

| Aspect | Stripe CLI | GitHub CLI (`gh`) | Vercel CLI | Next.js |
|---|---|---|---|---|
| **Default** | On (opt-out) | On (opt-out) | On (opt-out, first-run notice) | On (opt-out) |
| **Consent model** | Env var only | Env var, config cmd, `DO_NOT_TRACK` | CLI cmd, env var, first-run notice | CLI cmd, env var |
| **Opt-out mechanism** | `STRIPE_CLI_TELEMETRY_OPTOUT=1` | `GH_TELEMETRY=false`, `DO_NOT_TRACK=true`, `gh config set telemetry disabled` | `vercel telemetry disable`, `VERCEL_TELEMETRY_DISABLED=1` | `next telemetry disable`, `NEXT_TELEMETRY_DISABLED=1` |
| **Debug/inspect mode** | Not documented | `GH_TELEMETRY=log` → stderr | `VERCEL_TELEMETRY_DEBUG=1` → stderr | `NEXT_TELEMETRY_DEBUG=1` → stderr |
| **Sampling** | Not documented | 1% per invocation | Not documented | Not documented |
| **Transport** | Dedicated telemetry service (`r.stripe.com`) | Detached subprocess → internal analytics | Subprocess → Vercel analytics | HTTP POST |
| **Data collected** | Command, flags, error types, perf, OS, AI agent detection | Command, flags, device ID, OS, arch, CI, TTY, agent | Commands, args (no values), OS | Commands, config, plugins |
| **NOT collected** | API keys, personal info, file paths, arg values, account data | Arg values (debated), file paths, credentials | Env vars, file paths, file contents, logs | Sensitive config, file paths |
| **Retention** | Not disclosed | Not disclosed | Referenced in privacy policy | Referenced in privacy policy |
| **CI detection** | AI agent detection | `CI`, `GITHUB_ACTIONS` env | Not documented | Not documented |

### Key industry patterns

1. **Opt-out is the norm** — all four major CLIs default to on. Two received community backlash
   (GitHub CLI, Next.js). The Go team's `gotelemetry` is the notable exception (opt-in).
2. **Multiple opt-out paths** — env var (CI/ephemeral), CLI command (persistent), `DO_NOT_TRACK`
   (cross-tool standard). Environment variables take precedence.
3. **Debug/log mode** — lets users inspect the exact payload before deciding. GitHub and Vercel
   both support this.
4. **Non-blocking transport** — all implementations use background/subprocess sending so telemetry
   never slows commands.
5. **Command-level granularity** — track *which* command was run and *which flags* were set, but
   never flag *values* or positional arguments (these can contain PII like domain names).
6. **AI agent detection** — Stripe CLI now detects whether it's invoked by Claude Code, Cursor,
   or Cline. This is increasingly relevant.

---

## Proposal

### Consent Model: Opt-out with first-run notice

```
gddy collects anonymous usage data to improve the developer experience.
Run `gddy telemetry disable` or set GDDY_TELEMETRY=false to opt out.
Learn more: https://developer.godaddy.com/en/docs/cli/telemetry
```

**Rationale:** Aligns with Stripe, GitHub, and Vercel conventions. An opt-in model would give
insufficient signal (Go's experience confirms this — only ~16% of users opt in). A first-run
notice with clear opt-out is the current industry standard.

**Consent hierarchy (highest precedence first):**

1. `DO_NOT_TRACK=1` or `DO_NOT_TRACK=true` — cross-tool standard
2. `GDDY_TELEMETRY=false` (or `0`, `disabled`) — app-specific env var
3. `CI=true` — auto-disable in CI environments (conservative default)
4. `gddy telemetry disable` → persisted in `~/.config/gddy/config.toml`
5. Default: **enabled**

### What to collect

| Field | Source | Example | PII risk |
|---|---|---|---|
| `command` | Middleware | `"domain:quote"` | None |
| `flags` | Arg names (not values) | `["--domain", "--currency"]` | None |
| `status` | ActivityEvent | `"ok"`, `"error"` | None |
| `error_code` | GddyError | `"NOT_FOUND"`, `"NETWORK_ERROR"` | None |
| `duration_ms` | ActivityEvent | `342` | None |
| `cli_version` | `CARGO_PKG_VERSION` | `"0.9.1"` | None |
| `engine_version` | cli-engine version | `"0.8.4"` | None |
| `os` | `std::env::consts` | `"macos"`, `"linux"` | None |
| `arch` | `std::env::consts` | `"aarch64"`, `"x86_64"` | None |
| `shell` | `$SHELL` | `"zsh"`, `"bash"` | Low |
| `is_tty` | `std::io::IsTerminal` | `true` / `false` | None |
| `is_ci` | `$CI` env var | `true` / `false` | None |
| `is_agent` | Agent detection (see below) | `"cursor"`, `"claude-code"`, `null` | None |
| `env` | ActivityEvent | `"production"`, `"test"` | None |
| `auth_type` | `account_type` | `"sso"`, `"api-key"` | Low |
| `invocation_id` | UUID per invocation | `"550e8400-..."` | None |
| `device_id` | Persistent random UUID | `"a1b2c3d4-..."` | Low* |

*\*`device_id` is a random UUID stored locally, not derived from hardware or user identity. It
enables aggregate session analysis (e.g., "how many distinct installations use X") without
identifying users. Can be reset by deleting `~/.config/gddy/telemetry-id`.*

### What NOT to collect

| Excluded | Reason |
|---|---|
| Flag/argument **values** | May contain domain names, email, API keys |
| File paths | May contain usernames, project names |
| Request/response bodies | Contains customer data |
| API keys, tokens, PATs | Credentials |
| Domain names | Customer PII |
| Error messages (free text) | May embed domain names, paths |
| Shopper ID / customer ID | PII |
| IP address | Not collected client-side; transport should strip |
| Stack traces | May contain file paths |

### AI Agent Detection

Detect whether the CLI is being invoked by an AI coding agent. Following Stripe CLI's precedent:

| Agent | Detection |
|---|---|
| Cursor | `CURSOR_TRACE_ID` or `CURSOR_SESSION` env var |
| Claude Code | `CLAUDE_CODE` env var |
| GitHub Copilot | `GITHUB_COPILOT` env var |
| Cline | `CLINE_TASK_ID` env var |
| Generic MCP | `MCP_CLIENT_ID` env var |
| Generic CI | `CI=true` env var |

Report as `is_agent: "cursor"` (agent name) or `is_agent: null` (human invocation).

---

## Architecture

### Overview

```
┌───────────────────────────────────────────────────────────┐
│  gddy CLI process                                         │
│                                                           │
│  main.rs                                                  │
│    └─ Cli::new(CliConfig)                                 │
│         .with_activity(Arc::new(TelemetrySink))    ◄──────│── NEW
│         .with_init_deps(telemetry_init)            ◄──────│── NEW
│         .with_on_shutdown(telemetry_flush)          ◄──────│── NEW
│                                                           │
│  ┌─ Middleware::run() ─────────────────────────────┐      │
│  │  1. Auth → 2. Authz → 3. Handler               │      │
│  │  4. emit_activity(ActivityEvent) ──► TelemetrySink     │
│  │     └─ consent check                            │      │
│  │     └─ PII scrub (strip arg values)             │      │
│  │     └─ enrich (os, arch, shell, agent, version) │      │
│  │     └─ buffer in memory                         │      │
│  └─────────────────────────────────────────────────┘      │
│                                                           │
│  on_shutdown hook:                                        │
│    └─ flush buffer                                        │
│    └─ spawn detached subprocess OR async POST             │
│                                                           │
└───────────────────────────────────────────────────────────┘
                         │
                         ▼ (non-blocking, fire-and-forget)
              ┌─────────────────────┐
              │  Telemetry Endpoint │
              │  (internal service) │
              └─────────────────────┘
```

### Component Design

#### 1. Consent Manager (`telemetry/consent.rs`)

```rust
pub enum TelemetryConsent {
    Enabled,
    Disabled,
    Debug,  // log to stderr, don't send
}

pub fn resolve_consent(config: &ConfigFile) -> TelemetryConsent {
    // 1. DO_NOT_TRACK=1 → Disabled
    // 2. GDDY_TELEMETRY=false → Disabled
    // 3. GDDY_TELEMETRY=log → Debug
    // 4. CI=true → Disabled
    // 5. config [telemetry].enabled = false → Disabled
    // 6. Default → Enabled
}
```

#### 2. Telemetry Sink (`telemetry/sink.rs`)

Implements cli-engine's `ActivityEmitter` trait:

```rust
pub struct TelemetrySink {
    consent: TelemetryConsent,
    buffer: Mutex<Vec<TelemetryEvent>>,
    context: TelemetryContext,  // os, arch, version, device_id, etc.
}

#[async_trait]
impl ActivityEmitter for TelemetrySink {
    async fn emit(&self, event: ActivityEvent) -> Result<()> {
        match self.consent {
            TelemetryConsent::Disabled => return Ok(()),
            TelemetryConsent::Debug => {
                eprintln!("[telemetry] {}", serde_json::to_string(&self.transform(event))?);
                return Ok(());
            }
            TelemetryConsent::Enabled => {
                let telemetry_event = self.transform(event);
                self.buffer.lock().expect("lock").push(telemetry_event);
            }
        }
        Ok(())
    }
}
```

The `transform` method:
- Strips all argument **values** (keeps only flag names)
- Strips free-text error messages (keeps only error codes)
- Enriches with static context (OS, arch, version, agent detection)
- Adds `invocation_id` and `device_id`

#### 3. Transport (`telemetry/transport.rs`)

**Non-blocking, fire-and-forget.** Two viable approaches:

| Approach | Pros | Cons |
|---|---|---|
| **Detached subprocess** (GitHub CLI pattern) | Never blocks; survives parent exit | Extra process spawn; platform-specific |
| **Async POST in on_shutdown** (with timeout) | Simpler; single process | Adds ~100ms to exit; can be dropped on SIGKILL |

**Recommendation:** Start with async POST in `on_shutdown` with a 200ms timeout. Move to
subprocess if latency becomes a concern.

```rust
// on_shutdown hook
async fn flush_telemetry(sink: Arc<TelemetrySink>) {
    let events = sink.drain();
    if events.is_empty() { return; }

    let _ = tokio::time::timeout(
        Duration::from_millis(200),
        post_events(&events),
    ).await;
    // Silently drop on timeout — telemetry must never block the user
}
```

#### 4. First-Run Notice (`telemetry/notice.rs`)

On first invocation (no `[telemetry]` section in config):

```
gddy collects anonymous usage data to improve the developer experience.
Run `gddy telemetry disable` or set GDDY_TELEMETRY=false to opt out.
Learn more: https://developer.godaddy.com/en/docs/cli/telemetry
```

After displaying, write `[telemetry] enabled = true, noticed = true` to config. This notice
appears once and never again.

#### 5. CLI Commands (`telemetry/commands.rs`)

```
gddy telemetry status    # Show current telemetry state
gddy telemetry enable    # Opt in (write config)
gddy telemetry disable   # Opt out (write config)
```

### File Structure

```
rust/src/telemetry/
├── mod.rs          # Module root, public API
├── consent.rs      # Consent resolution logic
├── sink.rs         # ActivityEmitter implementation
├── transport.rs    # HTTP POST / subprocess flush
├── notice.rs       # First-run notice
├── context.rs      # Static context (OS, arch, agent detection, device_id)
├── scrub.rs        # PII scrubbing / arg-value stripping
└── commands.rs     # gddy telemetry enable/disable/status
```

### Integration Points in main.rs

```rust
// main.rs — additions marked with ◄

use crate::telemetry;

async fn main() -> ExitCode {
    tracing_subscriber::fmt()...init();

    let telemetry_sink = telemetry::init();  // ◄ resolve consent, load device_id

    let cli = Cli::new(
        CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
            // ... existing config ...
            .with_activity(telemetry_sink.clone())      // ◄ wire into middleware
            .with_pre_run(Arc::new(move |_mw, _path, _args| {
                update::maybe_spawn_background_refresh();
                telemetry::maybe_show_first_run_notice();  // ◄ one-time notice
                Ok(())
            }))
            .with_on_shutdown(Arc::new(move || {
                update::maybe_print_update_notice();
                telemetry::flush(telemetry_sink.clone());  // ◄ send buffered events
            }))
            .with_modules(all_modules()),
    );

    cli.execute().await
}
```

---

## Telemetry Event Schema

```json
{
  "schema_version": 1,
  "invocation_id": "550e8400-e29b-41d4-a716-446655440000",
  "device_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "timestamp": "2026-08-13T15:04:05.123Z",
  "app": "gddy",
  "cli_version": "0.9.1",
  "engine_version": "0.8.4",

  "command": "domain:quote",
  "flags": ["--domain", "--currency", "--json"],
  "status": "ok",
  "error_code": null,
  "duration_ms": 342,

  "env": "production",
  "auth_type": "sso",
  "is_streaming": false,

  "os": "macos",
  "arch": "aarch64",
  "shell": "zsh",
  "is_tty": true,
  "is_ci": false,
  "is_agent": null
}
```

**Error example:**

```json
{
  "command": "platform:app:deploy",
  "flags": ["--follow"],
  "status": "error",
  "error_code": "NETWORK_ERROR",
  "duration_ms": 30012,
  "is_agent": "cursor"
}
```

---

## What We Can Learn

### Product questions telemetry answers

| Question | Telemetry signal |
|---|---|
| Which commands are most/least used? | `command` frequency |
| What's breaking for users? | `status: error` + `error_code` by command |
| Are new features being adopted? | Command frequency over time |
| How fast are commands? | `duration_ms` percentiles |
| Are AI agents using the CLI differently? | `is_agent` vs. human usage patterns |
| Should we deprecate a command? | Zero/near-zero usage |
| Which environments matter? | `os` + `arch` + `shell` distribution |
| Do users hit auth issues often? | `status: auth-error` rate |
| Are streaming commands reliable? | `is_streaming` + error rates |
| Is CI usage growing? | `is_ci` trend |

### Derived metrics (server-side aggregation)

- **Daily/weekly active devices** — distinct `device_id` count
- **Command popularity ranking** — sorted by invocation count
- **Error rate by command** — `error / total` per command path
- **P50/P95/P99 latency by command** — `duration_ms` percentiles
- **Agent adoption curve** — `is_agent` percentage over time
- **Feature adoption funnel** — new command usage in first N days after release

---

## Privacy & Compliance

### Principles

1. **No PII.** Never collect argument values, domain names, file paths, credentials, or
   free-text error messages.
2. **Transparent.** Users can inspect exact payloads with `GDDY_TELEMETRY=log`.
3. **Controllable.** Multiple opt-out paths: env var (ephemeral), CLI command (persistent),
   `DO_NOT_TRACK` (cross-tool standard).
4. **Non-blocking.** Telemetry failures never affect command execution. Timeouts are aggressive.
5. **Minimal.** Collect only what's needed for product decisions. No speculative "collect
   everything" approach.
6. **Documented.** Public documentation page explaining what's collected, what's not, and how
   to opt out.

### Compliance considerations

| Concern | Mitigation |
|---|---|
| GDPR (EU users) | No PII collected; `device_id` is random UUID (not derived from personal data); opt-out available |
| Enterprise policies | `DO_NOT_TRACK` and env var support; auto-disable in CI |
| Data retention | Server-side: define retention policy (recommend 90 days raw, 1 year aggregated) |
| Data access | Internal analytics only; no third-party sharing of raw events |
| Security review | Transport over HTTPS; no sensitive data in payloads |

### `device_id` rationale

- Random UUID generated on first run, stored in `~/.config/gddy/telemetry-id`
- **Not** derived from: MAC address, hostname, username, hardware serial, or any PII
- Can be reset by deleting the file
- Used only for aggregate analysis (distinct device counts), not user identification
- cli-engine identity fields (`identity`, `sub`) are **stripped** before telemetry emission

---

## Comparison with Stripe CLI (closest analog)

| Aspect | Stripe CLI | gddy (proposed) |
|---|---|---|
| Default | On | On (with first-run notice) |
| Opt-out | Env var only | Env var + CLI command + `DO_NOT_TRACK` + CI auto-disable |
| AI agent detection | Yes (Cursor, Claude Code, Cline) | Yes (same + MCP) |
| Sampling | Not documented | Not initially (add if volume demands) |
| Debug/log mode | Not documented | Yes (`GDDY_TELEMETRY=log`) |
| What's collected | Commands, flags, errors, perf, OS, agent | Same |
| What's NOT collected | Keys, PII, paths, arg values, account data | Same |
| Transport | Dedicated service (`r.stripe.com`) | Internal endpoint (TBD) |
| First-run notice | No | Yes |

**gddy improves on Stripe's approach** by adding: first-run notice, `DO_NOT_TRACK` support,
CI auto-disable, debug/log mode, and a CLI command for persistent consent management.

---

## Rollout Plan

### Phase 1 — Infrastructure (cli-engine changes: none required)

All telemetry infrastructure lives in `gddy`, not cli-engine. The framework already provides
`ActivityEmitter`, `Auditor`, `ActivityEvent`, and middleware hooks. No cli-engine changes
are needed for Phase 1.

**Deliverables:**
- [ ] `telemetry/` module in gddy
- [ ] `TelemetrySink` implementing `ActivityEmitter`
- [ ] Consent manager with env var + config + `DO_NOT_TRACK` support
- [ ] PII scrubber (strip arg values, error messages, identity)
- [ ] First-run notice
- [ ] `gddy telemetry enable/disable/status` commands
- [ ] Debug mode (`GDDY_TELEMETRY=log`)
- [ ] Transport (async POST with timeout)
- [ ] Device ID management
- [ ] Unit tests for consent resolution, PII scrubbing, event transformation
- [ ] Integration tests confirming telemetry doesn't affect command behavior
- [ ] Public documentation page

### Phase 2 — Extended Coverage

- [ ] Emit events for built-in commands (help, tree, guide, completion) via `pre_run` hook
- [ ] Track clap parse errors / unknown commands
- [ ] Add sampling if event volume is high
- [ ] AI agent detection refinement based on real-world patterns
- [ ] Server-side dashboards and alerting

### Phase 3 — Advanced Analytics (optional)

- [ ] Feature flag integration (track exposure → usage)
- [ ] A/B testing support (command variants)
- [ ] Funnel analysis (e.g., `auth login` → first command → repeat usage)
- [ ] OpenTelemetry bridge for distributed tracing (if needed for platform debugging)

---

## Open Questions

1. **Telemetry endpoint** — where do events go? Options: internal analytics service, Datadog,
   a GoDaddy-hosted collector. This must be decided before implementation.

2. **Opt-out vs. opt-in default** — the industry norm is opt-out (on by default), but GitHub
   CLI faced backlash. Given `gddy` is an internal-facing developer tool (not a mass-market
   open-source CLI), opt-out is likely acceptable. Confirm with legal/privacy team.

3. **Sampling** — start without sampling (expect low volume from a developer CLI). Add 10%
   or 1% sampling only if volume becomes a cost concern.

4. **cli-engine enhancements** — should telemetry infrastructure (consent, transport, scrubbing)
   move into cli-engine as a reusable module for other GoDaddy CLIs? Or keep it in `gddy` only?
   Recommend: build in `gddy` first, extract to cli-engine if other CLIs need it.

5. **Retention policy** — how long to keep raw events? Recommend 90 days raw, 1 year aggregated.

6. **`device_id` vs. no `device_id`** — GitHub CLI's persistent `device_id` drew criticism.
   Alternative: session-only UUID (no persistence). Trade-off: lose "distinct installations"
   metric. Recommend: persistent with easy reset (delete file).

---

## References

- [Stripe CLI Telemetry](https://docs.stripe.com/cli/telemetry)
- [GitHub CLI Telemetry](https://docs.github.com/en/github-cli/github-cli/github-cli-telemetry)
- [Vercel CLI Telemetry](https://vercel.com/docs/cli/about-telemetry)
- [Next.js Telemetry](https://nextjs.org/telemetry)
- [Go Transparent Telemetry (opt-in)](https://research.swtch.com/telemetry-opt-in)
- [6 Telemetry Best Practices for CLI Tools](https://marcon.me/articles/cli-telemetry-best-practices/)
- [DO_NOT_TRACK Standard](https://consoledonottrack.com/)
- cli-engine `ActivityEmitter` / `Auditor` traits (`src/middleware.rs`)
- cli-engine middleware design (`docs/design.md`)
