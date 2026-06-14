# Universal pane→session binding — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax. Every Rust task is TDD: failing test first, run it red, implement, run it green, commit. Run the narrow test via PowerShell foreground on Windows (CLAUDE.md), 90s cap.

**Goal:** Bind every pane to its EXACT agent session via a provider-reported channel (claude hook + codex SessionStart hook + opencode plugin) — never cwd/mtime guessing — and parse codex's rollout format so codex chat-UI populates.

**Architecture:** Each provider reports its session id from inside the running agent, carrying `CODETERM_PANE_ID` + a per-launch nonce from env, to a generic `SessionBindingReport` route. Core persists only reported/preselected ids; stores are read by exact id. `detect_*` is demoted to session-search only and never assigns `pane.agent_session_id`.

**Tech Stack:** Rust (src-tauri, AgentProvider trait), the CodeTerm CLI (`codeterm hook ...` helper), a JS OpenCode plugin (`@opencode-ai/plugin`), axum routes, rusqlite, the pane_chat tailer.

**Spec:** `docs/superpowers/specs/2026-06-13-universal-session-binding-design.md`

**Verification discipline (user: "тестируй чтоб не ломалось больше"):** after EACH task, run the task test + `./build.sh check` + the full `sessions`/`pane_chat`/`parity_test` modules to catch regressions. No task is "done" until those are green on Windows.

---

## File Structure

- `src-tauri/src/agent/provider.rs` — add `SessionBindingReport`, `SessionBindingCapability`, trait methods (`session_binding_capability`, `ensure_session_binding_integration`, `build_session_binding_env`); keep `detect_*` (read-only).
- `src-tauri/src/api/hook_routes.rs` — generalize `hook_session_start` into the provider-agnostic report route (claude back-compat).
- `src-tauri/src/plugin/agent_codex/provider.rs` — implement `session_jsonl_path`, `parse_session_delta`, `session_binding_capability`/`ensure_session_binding_integration` (install `~/.codex/hooks.json`), `build_session_binding_env`.
- `src-tauri/src/plugin/agent_codex/rollout.rs` (NEW) — codex rollout JSONL → `RichMessage` parser + the `state_5.sqlite.threads` path resolver. Keep <1000 lines.
- `cli/src/cmd/hook.rs` (NEW) — `codeterm hook session-start --provider <p> --pane <id>`: read stdin JSON, POST a `SessionBindingReport`.
- `src-tauri/src/plugin/agent_opencode/provider.rs` — `session_binding_capability` (install the JS plugin), `build_session_binding_env`.
- `src-tauri/src/plugin/agent_opencode/plugin_template/` (NEW) — the `@opencode-ai/plugin` JS that reports `session.created`/`tui.session.select`.
- `src-tauri/src/service/pty_io.rs` + the spawn paths — inject `CODETERM_SESSION_BINDING_NONCE`, `CODETERM_AGENT_PROVIDER`, `CODETERM_API_URL`, `CODETERM_API_TOKEN`.
- `src-tauri/src/service/pane.rs` — remove pane-session WRITES from `kick_off_session_detection` / `refresh_session_model_once`; reads of exact ids only.
- `src-tauri/src/api/pane_chat_routes.rs` — route structured providers (opencode) through `chat_start_core` instead of JSONL-only.
- Parity/integration tests under `src-tauri/src/plugin/js_runtime/parity_test.rs` + `agent_codex` + `service/pane_chat`.

---

## PHASE 1 — Codex chat-UI unblock (highest value)

### Task 1: Generic report route accepts a codex `SessionBindingReport`

**Files:** Modify `src-tauri/src/agent/provider.rs` (add `SessionBindingReport` struct), `src-tauri/src/api/hook_routes.rs:288` (`hook_session_start`). Test: inline `#[cfg(test)]` in hook_routes (move body to `hook_routes/tests.rs` if it crosses the inline-test rule).

- [ ] **Step 1 — failing test:** a POST with `{provider_id:"codex", session_id:"<uuid>", source:"startup", binding_nonce:"N"}` to `/api/panes/<pane>/hook/session_start`, where the pane has `agent_type=None` and nonce `N`, sets `agent_session_id` to the uuid and `agent_type` to "codex". A second POST with a different `provider_id` than the now-bound pane is REJECTED. A POST whose nonce ≠ the pane nonce is REJECTED.
- [ ] **Step 2 — run red.** `cargo test --manifest-path src-tauri/Cargo.toml hook_session_start` → FAIL.
- [ ] **Step 3 — implement:** parse the generic body (provider_id/session_id/source/cwd/transcript_path/binding_nonce) alongside the legacy claude body (claude body has no provider_id → default to the pane's agent_type or "claude"). Validate provider exists; set/require agent_type match; require nonce match when the pane has a nonce; keep the existing atomic duplicate-session guard; `set_agent_session_id`; restart the tailer on change. Do NOT branch core logic on the literal "codex".
- [ ] **Step 4 — run green.** Same command → PASS.
- [ ] **Step 5 — regression gate:** `./build.sh check` + `cargo test --manifest-path src-tauri/Cargo.toml hook_ pane_chat sessions` all green.
- [ ] **Step 6 — commit:** `fix(session): generic SessionBindingReport route accepts any provider's hook (claude back-compat) (#165)`

### Task 2: `codeterm hook session-start` CLI helper

**Files:** Create `cli/src/cmd/hook.rs`; modify `cli/src/args/cli.rs` (subcommand) + `cli/src/main.rs` (dispatch). Test: `cli/src/cmd/hook.rs` `#[cfg(test)]`.

- [ ] **Step 1 — failing test:** given a stdin JSON `{"session_id":"X","cwd":"C","source":"startup","model":"m"}` and env `CODETERM_PANE_ID=P`, `CODETERM_SESSION_BINDING_NONCE=N`, the helper builds the correct `SessionBindingReport` JSON body (provider_id from `--provider`, pane from `--pane`/env, nonce from env) and targets `POST {CODETERM_API_URL}/api/panes/P/hook/session_start`. Test the body-builder pure function (don't hit the network).
- [ ] **Step 2 — run red.** `cargo test --manifest-path cli/Cargo.toml hook::` → FAIL.
- [ ] **Step 3 — implement:** `codeterm hook session-start --provider <p> [--pane <id>]` reads stdin, extracts fields, builds the body via a pure `fn build_report_body(...)`, POSTs with the auth header when `CODETERM_API_TOKEN` is set. Fire-and-forget; non-zero only on bad args.
- [ ] **Step 4 — run green.** PASS. Also regenerate CLI docs: `cargo run --manifest-path cli/Cargo.toml -- cli-manifest --write` (keeps #23 freshness tests green).
- [ ] **Step 5 — regression gate:** `cargo test --manifest-path cli/Cargo.toml` (incl. freshness) green.
- [ ] **Step 6 — commit:** `feat(cli): codeterm hook session-start — provider-agnostic session reporter (#165)`

### Task 3: Codex installs its SessionStart hook + reports

**Files:** Modify `src-tauri/src/plugin/agent_codex/provider.rs` (`ensure_session_binding_integration`, `build_session_binding_env`, `session_binding_capability`). Test: `agent_codex` `#[cfg(test)]`.

- [ ] **Step 1 — failing test:** `ensure_session_binding_integration(api_base)` writes a `~/.codex/hooks.json` (to a temp `$CODEX_HOME`) containing a `SessionStart` hook with matchers `startup,resume,clear,compact` whose command invokes `codeterm hook session-start --provider codex --pane $CODETERM_PANE_ID`. Idempotent (re-run = no dup). `build_session_binding_env(pane,nonce)` returns `CODETERM_PANE_ID`, `CODETERM_SESSION_BINDING_NONCE`, `CODETERM_AGENT_PROVIDER=codex`.
- [ ] **Step 2 — run red / Step 3 — implement** (resolve `~/.codex` via the existing sessions_root helper's parent; merge into an existing hooks.json without clobbering user hooks) / **Step 4 — run green.**
- [ ] **Step 5 — wire-up:** call `ensure_session_binding_integration` at codex pane launch (the spawn path) and merge `build_session_binding_env` into the PTY env. Regression gate: `./build.sh check` + agent_codex tests green.
- [ ] **Step 6 — commit:** `feat(codex): install SessionStart hook for authoritative session binding (#165)`

### Task 4: Codex rollout parser — `session_jsonl_path` + `parse_session_delta`

**Files:** Create `src-tauri/src/plugin/agent_codex/rollout.rs`; modify `agent_codex/provider.rs` (override both trait methods). Test: `rollout.rs` `#[cfg(test)]` with a real fixture committed under `agent_codex/fixtures/rollout_sample.jsonl` (captured from `~/.codex/sessions`).

- [ ] **Step 1 — failing test:** `parse_rollout_delta(fixture_bytes, 0)` returns `RichMessage`s mapping: `response_item.message` user/assistant/developer → text blocks; `response_item.reasoning` → thinking; `response_item.function_call` → tool_use (call_id,name,arguments); `response_item.function_call_output` → tool_result by call_id; `session_meta`/`turn_context`/`event_msg.token_count` produce NO chat message. Offset advances past complete lines only (partial last line held back). `session_jsonl_path(cwd, sid)` resolves the rollout path via `state_5.sqlite.threads.rollout_path` (exact id) with a recursive `rollout-*-<sid>.jsonl` filename fallback.
- [ ] **Step 2 — run red / Step 3 — implement** (mirror the JsProvider delta semantics: fault-tolerant, offset-advancing) / **Step 4 — run green.**
- [ ] **Step 5 — regression + live verify:** `./build.sh check` + sessions/pane_chat/agent_codex green; then on a real codex pane (bound via Task 3) confirm `GET /api/panes/<codex>/chat` now returns messages matching the terminal.
- [ ] **Step 6 — commit:** `feat(codex): parse rollout JSONL into chat messages — codex chat-UI populates (#165)`

**PHASE 1 EXIT GATE:** open a codex pane → its chat-UI shows the conversation (matches terminal) → two codex panes in one cwd each show ONLY their own session. Demo to Andrey before Phase 2.

---

## PHASE 2 — Universal seam generalization

### Task 5: Per-launch nonce + env injection for ALL spawned + manual panes

**Files:** Modify the PTY spawn path (`src-tauri/src/service/pty_io.rs` / `general_agent/lifecycle.rs` / wherever pane env is assembled) + `service/pane.rs` (store the per-pane nonce). Test: spawn-path unit test.

- [ ] Failing test: a freshly spawned pane gets a stored per-launch nonce and its PTY env contains `CODETERM_SESSION_BINDING_NONCE`, `CODETERM_AGENT_PROVIDER`, `CODETERM_API_URL`, and `CODETERM_API_TOKEN` (when configured). Manual shells inherit them. → implement → green → `./build.sh check` → commit `feat(session): per-launch binding nonce + reporter env on every pane`.

### Task 6: Migrate claude hook onto the generic report shape

**Files:** `agent_claude` + `hook_routes.rs`. Test: claude hook still binds; nonce now enforced when present.

- [ ] Failing test: the existing claude SessionStart body still binds (back-compat) AND, when the pane has a nonce, a claude report without/with-wrong nonce is rejected. → implement (normalize claude body → `SessionBindingReport`; keep preassigned `--session-id`) → green → regression gate → commit `refactor(claude): report session via the generic SessionBindingReport path`.

### Task 7: Demote `detect_*` — never assign a guessed id

**Files:** `service/pane.rs` (`kick_off_session_detection`, `refresh_session_model_once`), `agent/registry.rs` (`ensure_agent_state`), chat-start preflights. Test: the "never binds a sibling" guarantees.

- [ ] Failing test: with two same-cwd panes, neither bound, and NO provider report, `kick_off_session_detection` leaves BOTH `agent_session_id == None` (pending), never assigns a sibling's session. Detection may still resolve model/title (read-only). → implement (strip the `set_agent_session_id(detect_*)` writes; reads of exact ids only) → green → run the W3 sessions tests (must stay green) → commit `fix(session): detection no longer assigns pane session ids — provider-reported only`.

### Task 8: OpenCode plugin integration

**Files:** `agent_opencode/provider.rs` + `agent_opencode/plugin_template/`. Test: plugin install is idempotent; report route binds an opencode `ses_...`.

- [ ] Failing test: `ensure_session_binding_integration` installs the OpenCode plugin into the user's opencode config (temp HOME) idempotently; a simulated `session.created` report (provider_id="opencode", session_id="ses_X", pane, nonce) binds the pane and `GET /chat` reads sqlite by exact id. → implement the JS plugin (`event` hook → POST via `CODETERM_API_URL` with `CODETERM_PANE_ID`/nonce) + the Rust installer → green → commit `feat(opencode): bind via plugin session.created events, not sqlite recency`.

### Task 9: REST `pane_chat_routes::start` routes structured providers

**Files:** `api/pane_chat_routes.rs:start`. Test: opencode pane via REST returns structured chat (not "provider has no JSONL").

- [ ] Failing test: `POST /api/panes/<opencode>/chat/start` returns the structured snapshot (routes through `chat_start_core`), not a JSONL-only error. → implement → green → regression gate → commit `fix(web): /chat REST supports structured (sqlite) providers`.

**PHASE 2 EXIT GATE:** all 9 tests from the spec's eval list pass (two same-cwd codex panes; two same-cwd opencode panes; stale-saved-id→fresh-report; /clear restarts tailer; missing hook → pending never sibling). Demo to Andrey.

---

## PHASE 3 — Claude chat-UI live-update lag (separate root cause)

### Task 10: Fix queued/needs-reload live updates on claude panes

**Files:** TBD by investigation (`service/pane_chat/{tailer,structured_sink,tauri_sink}.rs`, the SSE `pane-chat-msg` path, frontend `useChatSession`).

- [ ] Use superpowers:systematic-debugging FIRST. Reproduce: claude pane, send a turn, observe chat-UI does not update live until reload. Gather evidence at boundaries (tailer emits? sink → SSE? frontend receives?). Find the layer that drops the live delta. Form one hypothesis, fix the root cause, add a regression test, verify live. Commit `fix(chat): claude chat-UI updates live without a reload`.

---

## Self-Review

- **Spec coverage:** seam (T1,T5,T6) ✓; codex hook (T3) ✓; codex parser (T4) ✓; opencode plugin (T8) ✓; detection demotion (T7) ✓; REST structured (T9) ✓; edge cases → Phase-2 exit eval list ✓; claude live-update (T10) ✓. No spec section unmapped.
- **Placeholders:** Task 10's files are genuinely TBD-by-debugging (correct — root cause not yet known); every other task names exact files + the test assertion. The codex rollout fixture must be captured from a real `~/.codex/sessions` file at execution time (noted in T4).
- **Type consistency:** `SessionBindingReport` fields are identical across T1/T2/T6/T8; `build_report_body` (T2) / `build_session_binding_env` (T3,T5) names are stable.

<!-- revision: 1 -->
