# Interactive Domain Wizard — Work Breakdown

Based on:
- [PR #193: Generalized Interactivity Design](https://github.com/godaddy/cli/pull/193)
- [INTERACTIVE_DOMAIN_WIZARD.md](./INTERACTIVE_DOMAIN_WIZARD.md)

**Total Effort:** ~78 hours (~10 working days)  
**Total PRs:** 6 (incremental, each independently mergeable)

---

## Dependency Graph

```
PR 1: Interactivity Framework ──┐
                                 ▼
PR 2: Wizard + Domain Register ──┬── PR 3: Contacts + Payment (parallel)
                                 ├── PR 4: Add-On Products (parallel)
                                 ├── PR 5: Multi-Entry Points (parallel)
                                 │
                                 └── PR 6: Polish + Docs (after 2, enhanced by 3-5)
```

PRs 3, 4, and 5 can be developed in parallel once PR 2 merges.

---

## PR 1: Generalized Interactivity Framework

**Effort:** ~16h  
**PR Title:** `feat: add generalized interactivity framework (--interactive flag + missing-input prompts)`  
**Deliverable:** Any command with a missing required arg prompts for it when in interactive mode (TTY). Scripts/agents get the existing error behavior unchanged.

### Tasks

- [x] Add `inquire` dependency to cli-engine Cargo.toml
- [x] Add global `--interactive` / `--non-interactive` flag to cli-engine's root clap command
- [x] Implement TTY auto-detection: default `--interactive` when stderr is a TTY and `CI` env var is unset
- [x] Create `InteractivityMode` enum (Interactive, NonInteractive) and thread through MiddlewareSnapshot
- [x] Create `prompt` module in cli-engine with helpers: `prompt_text()`, `prompt_select()`, `prompt_confirm()`, `prompt_multi_select()`
- [x] Implement missing-input interception: when clap returns `MissingRequiredArgument` and mode is Interactive, iterate over missing args and prompt
- [x] Auto-detect prompt type from clap arg metadata: `possible_values` → Select, bool → Confirm, free text → Input
- [x] Respect arg declaration order for prompt sequence (documented convention)
- [x] On cancel mid-prompt: show resume command with already-supplied flags
- [x] **Unit test:** TTY detection returns correct mode for TTY/non-TTY/CI
- [x] **Unit test:** Prompt type inference from clap arg metadata (possible_values, bool, free text)
- [x] **Integration test:** Missing required arg + interactive mode → prompts (mocked stdin)
- [x] **Integration test:** Missing required arg + non-interactive mode → error with helpful message
- [x] **Integration test:** All args supplied + interactive mode → no prompts, executes directly
- [x] **Integration test:** Cancel mid-prompt → shows resume command
- [x] `cargo fmt --check && cargo clippy -- -D warnings && cargo test`

---

## PR 2: Wizard Step Framework + Domain Register Command

**Effort:** ~24h  
**PR Title:** `feat(domain): add interactive domain register wizard (discovery, options, quote, execute)`  
**Deliverable:** Users can run `gddy domain register` and be walked through search → configure → confirm → buy in one session. Non-interactive fallback works with all flags.

### Tasks

- [x] Add `dialoguer`, `console`, `indicatif` to gddy Cargo.toml
- [x] Create `rust/src/domain/register/` directory structure: `mod.rs`, `wizard.rs`, `steps/{mod,discovery,options,review,execute}.rs`
- [x] Define `WizardState` struct (domain, available, period, privacy, auto_renew, nameservers, quote_token, price, etc.)
- [x] Define `StepResult` enum (Continue, Back, Cancel) and `WizardStep` trait
- [x] Implement `run_wizard()` step sequencer with forward/back navigation
- [x] Define `StepContext` (credential, env, debug, is_interactive, term)
- [x] Implement Discovery step: prompt for domain, call `/v3/domains/available`, show suggestions if taken, progressive pagination (5→15→25→50)
- [x] Implement Options step: period Select, privacy Confirm, auto-renew Confirm, custom NS Input
- [x] Implement Review step: call quote API, fetch agreements, display order summary, confirm prompt
- [x] Implement Execute step: call register API, show spinner, display success + next_actions
- [x] Create `RegisterArgs` struct with all CLI flags (`--period`, `--privacy`, `--agree`, `--confirm`, `--non-interactive`, etc.)
- [x] Implement TTY detection + non-interactive fallback (map flags → WizardState → execute directly)
- [x] Wire into domain group in `domain/mod.rs`
- [x] Implement human-friendly output for interactive mode (colored summary, not JSON)
- [ ] **Unit test:** `run_wizard` with mock steps (Continue, Back, Cancel navigation)
- [ ] **Unit test:** Domain name validation rejects invalid inputs
- [ ] **Unit test:** Suggestion deduplication + MAX_SUGGESTIONS cap
- [ ] **Integration test (mocked HTTP):** Non-interactive full flow → exit 0 with domain in result
- [ ] **Integration test (mocked HTTP):** Missing required flags in non-interactive → helpful error
- [ ] **Integration test (mocked HTTP):** `--dry-run` shows preview without charging
- [ ] `cargo fmt --check && cargo clippy -- -D warnings && cargo test`

---

## PR 3: Contacts Step + Payment Verification Gate

**Effort:** ~10h  
**PR Title:** `feat(domain): add contacts step + payment verification gate to domain register wizard`  
**Deliverable:** Wizard loads contacts from `contacts.toml`, offers account default or manual entry with save-to-file, and verifies payment before executing.

### Tasks

- [ ] Implement Contacts step: check `contacts::load()`, offer reuse if exists
- [ ] Implement "Use account default" path (`state.contacts = None` → omit from request)
- [ ] Implement interactive contact collection: all required fields with validation
- [ ] Implement phone number validation using `phonenumber` crate
- [ ] Implement country code validation (two-letter ISO shape check)
- [ ] Implement `save_contact_to_file()` — write TOML to `~/.config/gddy/contacts.toml`
- [ ] Wire contacts into quote API request body
- [ ] Implement Payment Verification step (Step 5b): call Shoppers API `GET /v1/shoppers/{id}/paymentMethods`
- [ ] Implement fail-open on 401/403 (let purchase step catch real error)
- [ ] Implement browser-open flow for missing payment + re-verify loop
- [ ] Non-interactive mode: fail immediately with clear error if no payment method
- [ ] **Unit test:** Phone validation accepts various formats, rejects garbage
- [ ] **Unit test:** Country validation accepts US/GB, rejects USA/123
- [ ] **Unit test:** `save_contact_to_file` roundtrip (write then `contacts::load()`)
- [ ] **Unit test:** Payment check 200+methods→true, 200+empty→false, 404→false, 401→true (fail-open)
- [ ] **Integration test (mocked HTTP):** Wizard with existing `contacts.toml` skips input
- [ ] **Integration test (mocked HTTP):** Payment method exists → continues to execution
- [ ] **Integration test (mocked HTTP):** Payment missing + non-interactive → error
- [ ] `cargo fmt --check && cargo clippy -- -D warnings && cargo test`

---

## PR 4: Add-On Products + Post-Registration Provisioning

**Effort:** ~12h  
**PR Title:** `feat(domain): add-on products (privacy, SSL, email) in domain register wizard`  
**Deliverable:** Step 4 offers a multi-select of add-on products. After registration succeeds, selected add-ons are provisioned with per-item success/failure reporting.

### Tasks

- [ ] Define `AddOn` struct (id, name, price, description) and `AVAILABLE_ADDONS` catalog
- [ ] Implement Add-Ons step: MultiSelect with privacy pre-selected if Step 2 chose privacy
- [ ] Implement `provision_privacy()`: `POST /v1/domains/{domain}/purchase/privacy` with consent
- [ ] Implement `provision_ssl_certificate()`: `POST /v1/certificates` with DV_SSL type
- [ ] Implement `provision_email()`: `POST /v1/email/domains/{domain}`
- [ ] Implement `execute_addons()` orchestrator: iterate, spinner per add-on, collect results
- [ ] Add-on failures don't fail the overall command (domain already registered)
- [ ] Add `--add <product>` repeatable flag for non-interactive mode
- [ ] Include add-on results in final JSON output (success/failure per product)
- [ ] **Unit test:** Empty selection → `state.addons` is empty
- [ ] **Unit test:** Privacy pre-selection logic based on `state.privacy`
- [ ] **Integration test (mocked HTTP):** 2 add-ons selected, one succeeds one fails → mixed results, exit 0
- [ ] **Integration test:** `--add privacy --add ssl` maps to `state.addons` correctly
- [ ] `cargo fmt --check && cargo clippy -- -D warnings && cargo test`

---

## PR 5: Multi-Entry Points (`--interactive` on suggest/available/quote)

**Effort:** ~8h  
**PR Title:** `feat(domain): multi-entry wizard (--interactive on suggest, available, quote)`  
**Deliverable:** `gddy domain suggest 'cool name' --interactive` fetches suggestions then enters the wizard. Same for `available` and `quote`.

### Tasks

- [ ] Add `--interactive` flag to `domain suggest` command
- [ ] After suggest results: inject into WizardState, call `run_wizard(start_at=0)` for pick-from-results
- [ ] Add `--interactive` flag to `domain available` command
- [ ] If available: inject domain, call `run_wizard(start_at=1)` for Options step
- [ ] If taken: inject suggestions, call `run_wizard(start_at=0)` for pick
- [ ] Add `--interactive` flag to `domain quote` command
- [ ] Inject quote token + price, call `run_wizard(start_at=4)` for Review step
- [ ] Add `start_at` parameter to `run_wizard()` to skip earlier steps
- [ ] **Integration test:** `domain available test.com --interactive` enters wizard at step 2
- [ ] **Integration test:** `domain suggest 'test' --interactive` enters wizard at step 1
- [ ] **Integration test:** `--interactive` without TTY → ignores flag, normal output
- [ ] `cargo fmt --check && cargo clippy -- -D warnings && cargo test`

---

## PR 6: Polish — Error Recovery, Progress, Documentation

**Effort:** ~8h  
**PR Title:** `feat(domain): wizard polish — error recovery, progress indicators, guide`  
**Deliverable:** Production-quality wizard with retry on network errors, step counter display, clean Ctrl+C exit, and `gddy guide domain-register`.

### Tasks

- [ ] Add step counter header to each step: "Step N of 6: \<name\>"
- [ ] Implement network retry with exponential backoff (3 attempts) for API calls
- [ ] Implement quote auto-refresh: if `quote_expires_at < now` before execution, re-quote
- [ ] Implement Ctrl+C handling: clean exit, no state persisted, no charge
- [ ] Add "Go back" option to Select prompts in Steps 2-5
- [ ] Create `gddy guide domain-register` markdown guide
- [ ] Update domain group long description to mention `register`
- [ ] Add `--interactive` flag documentation to suggest/available/quote help text
- [ ] **Unit test:** Retry logic (first fails, second succeeds → success)
- [ ] **Unit test:** Retry logic (all 3 fail → error)
- [ ] **Unit test:** Quote expiry detection and auto-refresh trigger
- [ ] **Manual test:** Full end-to-end in OTE environment
- [ ] `cargo fmt --check && cargo clippy -- -D warnings && cargo test`

---

## Manual Testing Checklist (end-to-end after all PRs merged)

| Scenario | Command | Expected |
|----------|---------|----------|
| Full wizard happy path | `gddy domain register` | Walk through all 6 steps, domain registered |
| Non-interactive with all flags | `gddy domain register example.com --period 1 --privacy --agree --confirm --non-interactive` | Registers without prompts |
| Entry from suggest | `gddy domain suggest "cool startup" --interactive` | Shows suggestions → enters wizard |
| Entry from available (taken) | `gddy domain available taken.com --interactive` | Shows alternatives → enters wizard |
| Missing flag non-interactive | `gddy domain register --non-interactive` | Error with guidance |
| Ctrl+C at any step | Ctrl+C during wizard | Clean exit, no side effects |
| No payment method | (remove payment) `gddy domain register` | Catches at Step 5b, opens browser |
| Piped input (no TTY) | `echo "test.com" \| gddy domain register` | Non-interactive error |
| JSON output override | `gddy domain register --output json` | JSON envelope output |
