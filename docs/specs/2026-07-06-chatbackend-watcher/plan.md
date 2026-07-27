# chatBackend Context Engines + Watcher Mode — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** chatBackend sessions gain two orthogonal knobs — a context engine (`chat` window vs `machine` state machine) and an interaction mode (`interactive` vs read-only `watcher`) — with an orchestration health watcher as the first machine+watcher application, LM Studio as the first implementing plugin, and the whole surface abstract enough that any future chatBackend plugin (OpenAI-compatible, websocket, …) is watcher-capable by construction.

**Architecture:** Contract types live in `packages/plugin-sdk`; the engine executor is a shared TS library in the `codeterm-plugins` repo bundled into each plugin; the host owns tick scheduling (extending the existing chatBackend poll loop), observation snapshots, verdict parsing, action execution with guardrails, state persistence on the Tab, read-only enforcement, and role/spawn/UI integration. Ticks are fire-and-forget enqueues (`watcherTick`), streamed by the existing `pump`/`poll` machinery; verdicts return as typed transcript messages intercepted by the structured sink. See `spec.md` next to this file for contracts and rationale.

**Tech stack:** Rust (daemon: `src-tauri/`), TypeScript (SDK `packages/plugin-sdk`, UI `src/`, plugin `codeterm-plugins/lmstudio`), QuickJS plugin runtime, existing `call_json` bridge.

**Repos:** Tasks 1–12 → `codeterm-canvas` (this repo, branch `feat/chatbackend-watcher-mode`); Task 12 produces the published npm package `@codeterm/chat-engine`. Tasks 13–15 → `codeterm-plugins` (separate repo/branch `feat/engine-watcher`), consuming `@codeterm/plugin-sdk` + `@codeterm/chat-engine` from npm (during development: `npm link` or a `file:` dep to the local checkout; publish before merging the plugins side). Task 16 spans both.

---

## File map

**codeterm-canvas**
- Modify: `packages/plugin-sdk/src/capabilities.ts` — SessionMode, ContextEngineConfig, WatcherTickInput, `watcherTick?`, capability flag
- Modify: `src-tauri/src/chatbackend/mod.rs` — `watcher_tick`, `watcher_capable` trait methods
- Modify: `src-tauri/src/plugin/js_runtime/chatbackend.rs` — JS adapter binding + manifest `watcher: true` parse
- Create: `src-tauri/src/chatbackend/tests_watcher.rs` (or extend `chatbackend/tests.rs`) — call_json contract test
- Modify: `src-tauri/src/service/role_profile.rs` — accept `watcher` profile
- Modify: `cli/src/args/agent.rs` — `SpawnRole::Watcher`, `--charter`, `--charter-file`, `--interval`
- Modify: `src-tauri/src/service/spawn_params.rs` — mode/engine into chatbackend spawn config + watcher-capability validation
- Modify: `src-tauri/src/service/pane_transport_chat.rs` — ctx promotion (`mode`, `engine`), read-only send rejection
- Modify: `src-tauri/src/multiplexer/types.rs` — `WatcherSpec` on Tab (persisted)
- Create: `src-tauri/src/service/watcher/mod.rs` — facade: spec types, scheduler hook, verdict apply
- Create: `src-tauri/src/service/watcher/observe.rs` — observation snapshot builder
- Create: `src-tauri/src/service/watcher/verdict.rs` — tolerant verdict parse + action guardrails/execution
- Modify: `src-tauri/src/service/spawn_chatbackend.rs` — poll-loop tick arming + verdict interception via sink
- Modify: `src/components/Terminal/PaneSessionChat.tsx` + `packages/chat-ui`/`chat-core` markers — composer hiding, context/verdict cards, status badge
- Modify: `docs/PLUGINS.md`, `docs/API.md` — document the surface (bump `<!-- revision: N -->`)

**codeterm-canvas (published package)**
- Create: `packages/chat-engine/` — `package.json` (`@codeterm/chat-engine`, publishable like plugin-sdk, version-synced), `src/index.ts` (engine executor: chat window policy + machine context assembly + verdict shaping), `src/index.test.ts`
- Modify: `scripts/sync-versions` flow (whatever `npm run sync:versions` drives) — include the new package; verify with `npm run check:versions`

**codeterm-plugins**
- Modify: `package.json` — add `@codeterm/chat-engine` devDependency (esbuild bundles it into `plugin.js`)
- Modify: `lmstudio/src/plugin.ts` — mode/engine in openSession, `watcherTick`, one-shot machine transport path, context_request emission
- Modify: `lmstudio/plugin.json` — `chatBackend: { spawnable: true, watcher: true }`
- Create: `lmstudio/prompts/watcher-orchestration.md` — canonical health-watch charter template
- Modify: `lmstudio/plugin.test.cjs` — watcher/engine tests

---

## Phase 1 — Contract (SDK + Rust trait + adapter)

### Task 1: SDK types

**Files:**
- Modify: `packages/plugin-sdk/src/capabilities.ts` (chatBackend section, ~lines 187–253)

- [ ] **Step 1: Add the types** — insert above `ChatBackendOpenSessionCtx`:

```ts
export type SessionMode = "interactive" | "watcher";

export interface ChatWindowConfig {
  maxMessages?: number;      // absent ⇒ unbounded (today's behavior)
  policy?: "top";            // v1: keep system prompt + last N; "llm" reserved
}

export interface ObservationConfig {
  orchestration?: boolean;   // pane-group snapshot (watchers default true)
  chatTail?: number;         // last N parsed messages per watched tab (0 = off)
  reports?: boolean;         // recent agent reports to the orchestrator
}

export type ContextEngineConfig =
  | { kind: "chat"; window?: ChatWindowConfig }
  | { kind: "machine"; charter: string; observations?: ObservationConfig; tickIntervalSecs?: number };

export interface WatcherTickInput {
  tick: number;
  nowMs: number;
  state: unknown;
  observations?: unknown;
}

export type WatcherVerdictStatus = "ok" | "attention" | "stalled";

export interface WatcherAction {
  kind: "nudge" | "notify" | "report";
  pane?: string;
  message: string;
}

export interface WatcherVerdict {
  status: WatcherVerdictStatus;
  summary: string;
  state?: unknown;
  actions?: WatcherAction[];
}
```

- [ ] **Step 2: Extend the session ctx and the interface** — `ChatBackendOpenSessionCtx` gains `mode?: SessionMode; engine?: ContextEngineConfig;`; the `ChatBackend` interface gains:

```ts
  // Enqueue one machine query; MUST return immediately (the pump/poll loop
  // streams the completion). Only called on sessions opened with mode "watcher".
  watcherTick?(sessionId: string, input: WatcherTickInput): void;
```

- [ ] **Step 3: Extend the capability declaration** — replace `ChatBackendCapability`:

```ts
export type ChatBackendCapability = boolean | { spawnable?: boolean; watcher?: boolean };
```

- [ ] **Step 4: Typecheck** — Run: `npx tsc --noEmit -p packages/plugin-sdk` (use the package's own check script if one exists). Expected: clean.
- [ ] **Step 5: Commit** — `git commit -m "feat(sdk): chatBackend session mode, context engine, watcher tick contract"`

### Task 2: Rust trait — `watcher_tick` + `watcher_capable`

**Files:**
- Modify: `src-tauri/src/chatbackend/mod.rs` (trait, after `set_model` ~line 122)

- [ ] **Step 1: Add trait methods** (defaults follow the `set_model` "unsupported" pattern):

```rust
    /// Whether this backend implements watcher mode (manifest `chatBackend: { watcher: true }`).
    fn watcher_capable(&self) -> bool {
        false
    }

    /// Enqueue one watcher tick (fire-and-forget; pump/poll streams the completion).
    /// `input` is the serialized WatcherTickInput. Default: unsupported.
    fn watcher_tick(
        &self,
        session_id: &str,
        input: serde_json::Value,
    ) -> Result<(), ChatBackendError> {
        let _ = (session_id, input);
        Err(ChatBackendError::Call(
            "this chatBackend does not support watcher mode".into(),
        ))
    }
```

- [ ] **Step 2: `./build.sh check`** — Expected: clean (defaults keep all impls compiling).
- [ ] **Step 3: Commit** — `git commit -m "feat(chatbackend): watcher_tick + watcher_capable on the provider trait"`

### Task 3: JS adapter binding + contract test

**Files:**
- Modify: `src-tauri/src/plugin/js_runtime/chatbackend.rs` (`JsChatBackend`; manifest spawnable parse ~line 53)
- Test: extend `src-tauri/src/chatbackend/tests.rs` (module `tests`; keep `mod tests;` + body file per repo convention)

- [ ] **Step 1: Write the failing contract test** — a fixture plugin JS exporting `watcherTick` that records its args; assert the adapter forwards `(sessionId, input)` via `call_json` and that a plugin *without* the export surfaces the default "unsupported" error. Follow the existing call_json contract tests in this module for the harness shape (the file already builds fixture plugins for openSession/poll).
- [ ] **Step 2: Run it, verify it fails** — `./build.sh test -- chatbackend::tests::watcher` (narrow filter, one run, per repo test discipline). Expected: FAIL (method missing).
- [ ] **Step 3: Implement** — in `JsChatBackend`: parse `watcher: true` from the manifest capability object next to `spawnable` (store `watcher_capable: bool`); implement `watcher_capable()` returning it; implement `watcher_tick` as `self.call_json("watcherTick", json!([session_id, input]))?` mapped to `()` like `send_message`.
- [ ] **Step 4: Run test, verify pass** — same command. Expected: PASS.
- [ ] **Step 5: Commit** — `git commit -m "feat(plugin): JsChatBackend watcherTick binding + call_json contract test"`

## Phase 2 — Role, spawn, persistence

### Task 4: `Manager` as a first-class role + `watcher` profile + CLI sugar

Manager stops being a UI illusion over `role_profile` and becomes a structural
role: `AgentRole::{Orchestrator, Manager, Worker}`. `role_profile` becomes the
manager's *specialization* (`planner` | `watcher`) — required and validated on
managers, rejected loudly when unknown (today unknown profiles silently
downgrade to generic Worker). No compat shims for previously persisted
Worker+planner Tabs (repo rule: delete legacy aggressively).

**Files:**
- Modify: `src-tauri/src/multiplexer/state/role.rs` — add `Manager` to `AgentRole` (serde string form `"Manager"`); invariants: Manager requires an Orchestrator parent; Manager cannot spawn children (same leaf rule as Worker); follow every existing invariant test pattern in that file
- Modify: `src-tauri/src/service/role_profile.rs:52-106` (`normalize_spawn_params`) — full rewrite of the match
- Modify: every `AgentRole` exhaustive match the compiler flags after adding the variant (`launch.rs::role_onboarding_task` `(Worker, planner)` arm → `(Manager, planner)`; `spawn.rs` planner branch; `orchestration_pair.rs` planner spawn; hierarchy/report paths; grep `AgentRole::` to enumerate)
- Modify: `cli/src/args/agent.rs` (`SpawnRole` enum ~line 233, flags ~lines 9-24)
- Test: role.rs invariant tests + `role_profile.rs` sibling tests (find with `grep -rn normalize_spawn_params src-tauri/src`)

- [ ] **Step 1: Failing tests** — (a) `{role: "Planner"}` and `{role: "Watcher"}` normalize to `role=Manager` + profile `planner`/`watcher`; (b) `{role: "Manager", role_profile: "planner"}` passes through; (c) `{role: "Manager"}` with missing or unknown profile is an **Err**, not a downgrade; (d) role.rs invariants: Manager without parent rejected, Manager spawning a child rejected, Manager under Orchestrator accepted.
- [ ] **Step 2: Implement** — normalization core:

```rust
        Some("Planner") | Some("planner") => {
            params.role = Some("Manager".into());
            params.role_profile = Some("planner".into());
        }
        Some("Watcher") | Some("watcher") => {
            params.role = Some("Manager".into());
            params.role_profile = Some("watcher".into());
        }
        Some("Manager") => match profile {
            Some("planner" | "Planner") => params.role_profile = Some("planner".into()),
            Some("watcher" | "Watcher") => params.role_profile = Some("watcher".into()),
            other => return Err(format!("Manager requires a known role_profile (planner|watcher), got {other:?}")),
        },
```

  (`normalize_spawn_params` becomes fallible — thread the `Result` to its callers; spawn surfaces the message verbatim.) Migrate planner spawn paths to `Manager` and update the sidebar-feeding role serialization so the Tab snapshot exposes `"Manager"`.
  CLI: add `Manager`/`Watcher` to `SpawnRole` and new flags `--charter <TEXT>`, `--charter-file <PATH>` (mutually exclusive; file read client-side), plus the policy trio — `--interval <DURATION>`, `--nudge-cooldown <DURATION>`, `--max-actions <N>` (durations parse like `send --delay`; all optional, host defaults + clamps apply). Pass through spawn params JSON as `charter` / `tick_interval_secs` / `nudge_cooldown_secs` / `max_actions_per_tick`.
- [ ] **Step 3: Frontend follows the role — and the sections must ACTUALLY render** (known live bug: sections currently don't show). Root cause to fix, not just taxonomy: in `src/components/Sidebar/SidebarWorkspaceTabs.tsx:188-214` the Managers section renders ONLY `plannerTab`, resolved via the plan binding (`resolvePlannerPeerTabContext`) — a manager Tab with no plan binding never appears, and `count`/`showHeader` are computed from `plannerTab ? 1 : 0` instead of from roles. Rework: `src/components/Sidebar/orchestrationSections.ts` — `sectionForRoleProfile` becomes `sectionForRole(role, profile)` keyed on `role === "Manager"`; delete `MANAGER_ROLE_PROFILES`; badge from profile (`PLANNER`/`WATCHER`). `SidebarWorkspaceTabs.tsx` — the Managers section renders ALL child tabs whose Tab role is `Manager` (planner keeps its peer-context special-casing for ordering, but membership comes from the role), counts/`showHeader`/group summaries come from role-based `countBySection`, and worker subtree excludes manager Tabs. Verify with component tests: an orchestrator with one Manager child (no plan binding) + two Workers renders both section headers with counts 1/2.
- [ ] **Step 4: Verify** — `./build.sh test -- role_profile` then `-- role` (one at a time); `cargo check` in `cli/`; frontend tests for the touched components.
- [ ] **Step 5: Commit** — `git commit -m "feat(roles): Manager as a first-class role; planner/watcher are manager profiles"`

### Task 5: Spawn config plumbing + watcher-capability validation

**Files:**
- Modify: `src-tauri/src/service/orchestrator/types.rs` (`SpawnAgentParams` — add `charter: Option<String>`, `tick_interval_secs: Option<u64>`, `nudge_cooldown_secs: Option<u64>`, `max_actions_per_tick: Option<u32>`)
- Modify: `src-tauri/src/service/role_profile.rs` (`from_json` — parse the two new fields)
- Modify: `src-tauri/src/service/spawn_params.rs:118-134` (`chatbackend_spawn_config`), `:163` (`classify_spawn_target`)
- Modify: `src-tauri/src/service/pane_transport_chat.rs:58` (`build_chatbackend_open_session_ctx`)
- Test: `src-tauri/src/api/pane_routes/tests_create_pane.rs` + spawn_params unit tests

- [ ] **Step 1: Failing tests** — (a) spawning with profile `watcher` + charter produces a chatbackend config `{ mode: "watcher", engine: { kind: "machine", charter, tickIntervalSecs } }`; (b) `build_chatbackend_open_session_ctx` promotes `mode`/`engine` from config to top-level ctx keys (like it does `systemPrompt`/`preset`/`model`); (c) `--role Watcher` against a non-watcher-capable provider (or a PTY provider) is rejected with a message naming the requirement.
- [ ] **Step 2: Verify fail** — `./build.sh test -- spawn_params` then `-- pane_transport_chat` (one at a time). Expected: FAIL.
- [ ] **Step 3: Implement** — extend `chatbackend_spawn_config(system_prompt, preset, model, mode, engine)`; watcher validation in `classify_spawn_target` via `ChatBackendRegistry` `watcher_capable()` (capability question, never a provider-id match); charter required when profile is watcher (reject otherwise: "watcher spawn requires --charter or --charter-file").
- [ ] **Step 4: Verify pass.** `_meta` persistence needs no work — openSession config is already written as the first JSONL line and reused on reopen.
- [ ] **Step 5: Commit** — `git commit -m "feat(spawn): thread mode/engine into chatBackend openSession config, gate watcher spawns on capability"`

### Task 6: `WatcherSpec` on the Tab

**Files:**
- Modify: `src-tauri/src/multiplexer/types.rs` (near `role_profile`, ~line 46; `PaneBacking` stays untouched)
- Test: `src-tauri/src/multiplexer/state/snapshot/tests.rs` (round-trip)

- [ ] **Step 1: Failing round-trip test** — Tab with `watcher: Some(WatcherSpec {...})` serializes and restores identically.
- [ ] **Step 2: Verify fail**, then **Step 3: Implement**:

```rust
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct WatcherPolicy {
    pub tick_interval_secs: u64,    // default 120, clamp 30..3600
    pub nudge_cooldown_secs: u64,   // default 600, clamp >= 60
    pub max_actions_per_tick: u32,  // default 3, clamp <= 5
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct WatcherSpec {
    pub charter: String,
    pub policy: WatcherPolicy,
    #[serde(default)]
    pub observations: serde_json::Value, // ObservationConfig JSON
    #[serde(default)]
    pub state: serde_json::Value,        // authoritative machine state, ≤16KB
    #[serde(default)]
    pub tick: u64,
    #[serde(default)]
    pub last_tick_ms: Option<u64>,
    #[serde(default)]
    pub last_status: Option<String>,     // "ok" | "attention" | "stalled"
    #[serde(default)]
    pub tick_inflight_since_ms: Option<u64>, // watchdog: clear on verdict; timeout ⇒ missed
}
```

  `Tab` gains `#[serde(default, skip_serializing_if = "Option::is_none")] pub watcher: Option<WatcherSpec>`. `spawn_chatbackend_impl` populates it from spawn params when profile is watcher; `WatcherPolicy::clamped(requested)` applies the defaults + bounds in one place (unit-test the clamps: interval 5s→30s, cooldown 0→60, max_actions 99→5).
- [ ] **Step 4: Verify pass** — `./build.sh test -- snapshot`. **Step 5: Commit** — `git commit -m "feat(mux): persisted WatcherSpec on tabs"`

## Phase 3 — Host watcher runtime

### Task 7: Observation snapshot builder

**Files:**
- Create: `src-tauri/src/service/watcher/mod.rs` (facade: `pub mod observe; pub mod verdict;` + re-exports)
- Create: `src-tauri/src/service/watcher/observe.rs`
- Test: `src-tauri/src/service/watcher/observe/tests.rs` (or inline `mod tests` + body file per convention)

- [ ] **Step 1: Failing tests** — given a fixture mux state (orchestrator O with children: planner P, watcher W (self), watcher W2, workers A/B), `build_observation(mux, w_pane_id, &config)` returns JSON containing O, P, A, B with `{pane_id, title, role, role_profile, status, last_activity_ms}` and NOT W or W2; `chatTail: 2` attaches last 2 parsed messages per included Tab; group resolution walks `parent_pane_id` up to the orchestrator.
- [ ] **Step 2: Verify fail** — `./build.sh test -- watcher::observe`. 
- [ ] **Step 3: Implement** — group = orchestrator (via parent chain) + all its descendants; exclude self + any tab with `role_profile == Some("watcher")`; statuses from the tab status tracker; chat tails via the parsed-chat service (same source as `codeterm tab chat`); reports from the report store addressed to the orchestrator. **Lock discipline:** snapshot mux state under the lock into plain structs, drop the guard, then do chat-tail/report reads — mux is lock #1, never hold it across other subsystem calls.
- [ ] **Step 4: Verify pass. Step 5: Commit** — `git commit -m "feat(watcher): observation snapshot builder (group minus self and other watchers)"`

### Task 8: Verdict parse + action guardrails

**Files:**
- Create: `src-tauri/src/service/watcher/verdict.rs`
- Test: sibling tests file

- [ ] **Step 1: Failing tests** — (a) parses naked JSON, fenced ```json blocks, and JSON with surrounding prose; (b) malformed input degrades to `{status: "attention", summary: <raw prefix>, actions: []}`; (c) guardrails: action targeting a tab outside the observed group is dropped (logged); >3 actions truncated; nudge to a tab nudged <cooldown ago is suppressed; state >16KB rejected keeping previous state; (d) unknown `status` degrades to `attention`.
- [ ] **Step 2: Verify fail** — `./build.sh test -- watcher::verdict`.
- [ ] **Step 3: Implement**:

```rust
pub struct VerdictOutcome {
    pub verdict: WatcherVerdict,          // typed mirror of the SDK shape
    pub allowed_actions: Vec<WatcherAction>,
    pub dropped: Vec<(WatcherAction, &'static str)>, // reason: out_of_group | cooldown | overflow
}

pub fn parse_verdict(raw: &str) -> WatcherVerdict { /* tolerant extract */ }
pub fn apply_guardrails(
    v: &WatcherVerdict,
    policy: &WatcherPolicy,          // spawner-set, host-clamped (Task 6)
    group: &HashSet<String>,
    cooldowns: &HashMap<String, u64>,
    now_ms: u64,
) -> VerdictOutcome { /* … */ }
```

  Nudge execution goes through `Messenger::deliver` with a watcher-origin envelope label; `notify` through the notification service; `report` through the hierarchy report path to the parent. Cooldown/max-actions come from `WatcherPolicy` (never hardcoded constants beyond the clamp bounds); the per-target cooldown timestamps live in a runtime map (not persisted; restart resets cooldowns — acceptable).
- [ ] **Step 4: Verify pass. Step 5: Commit** — `git commit -m "feat(watcher): tolerant verdict parsing + host-enforced action guardrails"`

### Task 9: Tick scheduling + verdict interception in the poll loop

**Files:**
- Modify: `src-tauri/src/service/spawn_chatbackend.rs` (poll loop `spawn_chatbackend_poll_loop:243`, structured sink wiring)
- Test: extend the poll-loop tests in the same module

- [ ] **Step 1: Failing tests** — (a) a watcher Tab whose `last_tick_ms + interval <= now` and whose session reports `done` gets exactly one `watcher_tick` call with `{tick, nowMs, state, observations}`; (b) no second tick while `tick_inflight_since_ms` is set; (c) a polled delta containing `type: "watcher_verdict"` updates `WatcherSpec` (state/status/tick counters, clears inflight) and invokes the action executor; (d) inflight older than max(2×interval, 300s) is cleared as a missed tick (logged, transcript note).
- [ ] **Step 2: Verify fail** — `./build.sh test -- spawn_chatbackend` (single narrow run).
- [ ] **Step 3: Implement** — on each poll-loop iteration for a watcher Tab: check due/idle → build observation (Task 7) → `watcher_tick` (fire-and-forget; on Err annotate transcript, back off one interval). In the sink path, scan new messages for `watcher_verdict` → Task 8 parse/guardrails → execute → persist spec. All plugin calls stay on the existing blocking thread; observation building must not hold the mux lock across the plugin call (snapshot-then-call).
- [ ] **Step 4: Verify pass. Step 5: Commit** — `git commit -m "feat(watcher): poll-loop tick scheduler + verdict interception"`

### Task 10: Read-only enforcement

**Files:**
- Modify: `src-tauri/src/service/pane_transport_chat.rs:91` (`ChatBackendTransport::send`)
- Test: module tests + `src-tauri/src/api/pane_routes/tests_create_pane.rs` neighbor for the REST path

- [ ] **Step 1: Failing test** — `send` to a Tab with `watcher: Some(_)` returns an error `"this tab is a watcher (read-only); its inputs come from the tick scheduler"`; the internal tick path is unaffected (it calls `watcher_tick`, never `send`).
- [ ] **Step 2: Verify fail** — `./build.sh test -- pane_transport_chat`.
- [ ] **Step 3: Implement** — guard at the top of the transport `send` (single choke point: CLI `codeterm send`, REST, UI all funnel here).
- [ ] **Step 4: Verify pass. Step 5: Commit** — `git commit -m "feat(watcher): reject sends into watcher tabs at the transport seam"`

## Phase 4 — UI

### Task 11: Watcher Tab UI (composer, cards, badge)

**Files:**
- Modify: `src/components/Terminal/PaneSessionChat.tsx` (~line 458 — model/preset bubbles row)
- Modify: `packages/chat-core/src/markers.ts` (context/verdict marker twins), `packages/chat-ui/src/MessageBubble*.tsx` (card rendering)
- Verify: `src/components/Sidebar/orchestrationSections.ts` — Task 4 Step 3 already moved sectioning to `role === "Manager"`; here only confirm a watcher Tab lands in Managers with a `WATCHER` badge
- Test: Playwright web-mode spec `tests/playwright/specs/` + component tests next to the touched files

- [ ] **Step 1: Component tests first** — (a) `backing.kind === "chat_backend"` + Tab `watcher` set ⇒ composer replaced by a static "Watcher — read-only" bar with the charter preview; (b) a `context_request` message renders as a collapsible card (reuse the system-prompt card pattern from `markers.ts`); (c) a `watcher_verdict` message renders a status card; Tab header shows a badge colored by `last_status` (`ok`→ok, `attention`→warn, `stalled`→danger tones).
- [ ] **Step 2: Verify fail, implement, verify pass** — `npx vitest run <touched test files>` (match the repo's existing test invocation for `src/`; check `package.json` scripts).
- [ ] **Step 3: Playwright, isolation first** — one spec: open a fixture watcher Tab (dev API), assert composer absent + charter card visible. Run ALONE first: `npx playwright test <file> -g "watcher tab is read-only" --timeout=30000 --reporter=line` against healthy dev (`curl -s http://127.0.0.1:7686/api/health` first). Caps mandatory.
- [ ] **Step 4: Commit** — `git commit -m "feat(ui): watcher tab read-only composer, context/verdict cards, status badge"`

## Phase 5 — Plugin side (`codeterm-plugins` repo)

### Task 12: `@codeterm/chat-engine` — published engine library (codeterm-canvas)

**Files:**
- Create: `packages/chat-engine/package.json` — name `@codeterm/chat-engine`, publishable (mirror `packages/plugin-sdk/package.json`: not private, repository/license/keywords, `main`/`types` → `src/index.ts` since plugin builds bundle TS source via esbuild), version in the app lockstep
- Create: `packages/chat-engine/src/index.ts`
- Create: `packages/chat-engine/src/index.test.ts`
- Modify: the `sync:versions` script target list — include the new package; `npm run check:versions` must pass

- [ ] **Step 1: Failing tests** — chat engine: `assembleChat(history, window)` returns system prompt + last `maxMessages` messages, never more; absent window ⇒ full history (today's behavior). Machine engine: `assembleMachine(charter, state, input)` returns exactly `[system: charter + verdict contract, user: render(state, input)]` — prior transcript never leaks in.
- [ ] **Step 2: Verify fail** — run with the same runner `packages/chat-core` tests use (check its scripts; do not invent a new harness). Zero runtime dependencies — pure functions over `EngineMessage[]`.
- [ ] **Step 3: Implement** (transport-agnostic — pure functions over messages):

```ts
export interface EngineMessage { role: "system" | "user" | "assistant"; content: string }

export function assembleChat(
  history: EngineMessage[], window?: ChatWindowConfig,
): EngineMessage[] { /* top policy: [system?, ...tail(maxMessages)] */ }

export function assembleMachine(
  charter: string, state: unknown, input: WatcherTickInput | { query: string },
): EngineMessage[] { /* [system: charter+verdictContract, user: JSON.stringify({state, input})] */ }
```

- [ ] **Step 4: Verify pass. Step 5: Version sync** — `npm run sync:versions && npm run check:versions`. Expected: clean.
- [ ] **Step 6: Commit** — `git commit -m "feat(chat-engine): published context engine library (chat window + machine assembly)"`
- [ ] **Step 7: Publish before the plugins-repo tasks merge** — `npm publish --access public` from `packages/chat-engine` (same flow as plugin-sdk releases); during local development the plugins repo may use `npm link @codeterm/chat-engine`.

### Task 13: LM Studio watcher mode

**Files:**
- Modify: `lmstudio/src/plugin.ts` — `Session` gains `mode`, `engine`; `resolveSession` (~line 1002) reads `ctx.mode`/`ctx.engine`; new `watcherTick` export; one-shot path in `startLmStudioCall` (~lines 736-801)
- Modify: `lmstudio/plugin.json` — `"chatBackend": { "spawnable": true, "watcher": true }`
- Test: `lmstudio/plugin.test.cjs`

- [ ] **Step 1: Failing tests** — (a) `openSession({mode: "watcher", engine: {kind: "machine", charter}})` emits the charter as the observable system-prompt card and does NOT emit the default preset prompt; (b) `watcherTick(sid, input)` enqueues a request whose body has NO `previous_response_id` and whose `input` messages are exactly `assembleMachine(...)` output (no history growth across two ticks — capture both request bodies and compare); (c) completed tick emits `context_request` (the assembled input, for the UI card), the streamed assistant message, and a final `watcher_verdict` message whose content is the model's JSON (verbatim; parsing/tolerance is host-side); (d) tool loop is skipped in watcher mode (a ```codeterm-tool block in the reply is NOT executed); (e) `sendMessage` on a watcher session is a no-op with a logged warning (belt-and-braces behind the host guard); (f) chat sessions with `engine.window.maxMessages: N` build assembled context via `assembleChat` — request contains ≤ N history messages; (g) `machine + interactive`: `sendMessage` on a machine-engine session routes through `assembleMachine(charter, state, { query: text })` — each user message is a query into the state machine, prior transcript never enters the request, and the response's verdict `state` advances the session state.
- [ ] **Step 2: Verify fail** — `node lmstudio/plugin.test.cjs`.
- [ ] **Step 3: Implement** — watcher path reuses `startLmStudioCall`'s streaming with: forced no-continuation (`previousResponseId` ignored), input = `assembleMachine`, `s.messages` used only as UI transcript (never as context source), tool parsing skipped, and on stream end append the `watcher_verdict` message. Chat path: apply `assembleChat` inside the existing `assembledContext` builder when a window is configured.
- [ ] **Step 4: Verify pass**, plus `npx tsc --noEmit` and `node scripts/build-plugin.mjs lmstudio`.
- [ ] **Step 5: Commit** — `git commit -m "feat(lmstudio): watcher mode + machine/chat context engines"`

### Task 14: Canonical health-watch charter

**Files:**
- Create: `lmstudio/prompts/watcher-orchestration.md`

- [ ] **Step 1: Write the charter template** — instructs: you observe an orchestration group snapshot (schema documented inline); progressing = recent activity / advancing statuses; stalled = no activity beyond thresholds, workers stuck on prompts, orchestrator idle with pending reports; respond ONLY with the verdict JSON; nudges are short, name the observed evidence, addressed to the stalled Tab. Include two worked examples (ok, stalled+nudge) — few-shot matters for small local models.
- [ ] **Step 2: Wire as default** — plugin uses this file as charter when a watcher spawn provides none… **No** — charter is required at spawn (Task 5); instead reference the file from `configHelp`/README as the copy-paste starting point, and add a `watcher-orchestration` entry under a new `charters:` key in `config.yaml` that `--charter charter:watcher-orchestration` can reference by id (CLI passes the literal string; the plugin resolves the `charter:` prefix from config).
- [ ] **Step 3: Test** — config-referenced charter resolves in `openSession`; unknown `charter:` id errors the spawn (surfaced via openSession failure teardown).
- [ ] **Step 4: Commit** — `git commit -m "feat(lmstudio): canonical orchestration health-watch charter"`

### Task 15: Plugin README + channel docs

**Files:**
- Modify: `lmstudio/README.md`, `lmstudio/plugin.json` `configHelp`

- [ ] **Step 1: Document** modes/engines, watcher spawn one-liner, charter config, verdict contract (link to host docs).
- [ ] **Step 2: Commit** — `git commit -m "docs(lmstudio): watcher mode + engines"`

## Phase 6 — Integration proof + host docs

### Task 16: End-to-end smoke + docs

**Files:**
- Create: `evals/watcher-health/` — `README.md` + `run.sh` (source `evals/_lib/lib.sh`, use `dc` only — never bare `codeterm`)
- Modify: `docs/PLUGINS.md` (chatBackend section, lines ~198-273), `docs/API.md` (bump `<!-- revision: N -->` in both)

- [ ] **Step 1: Bench script** — against dev (7686): create workspace → spawn orchestrator (claude) + one worker with a trivial stalling task → install dev-built lmstudio plugin → spawn watcher (`--role Watcher --charter charter:watcher-orchestration --interval 30s --parent <orch>`) → wait 2 intervals → assert via `dc tab chat --tab <watcher>`: context card, verdict message present; assert orchestrator received ≤1 watcher-marked nudge (cooldown holds across the 2nd tick). Requires LM Studio running locally with a loaded model; script degrades to SKIP with a clear message when `curl -s localhost:1234/api/v1/models` fails.
- [ ] **Step 2: Run it** — `evals/watcher-health/run.sh`; iterate until green. This is the acceptance-criteria oracle (spec §Acceptance 1-3).
- [ ] **Step 3: Host docs** — PLUGINS.md: engine/mode/`watcherTick`/verdict contract + manifest flag; API.md: spawn params additions. Bump revisions.
- [ ] **Step 4: Commit** — `git commit -m "feat(watcher): e2e health-watch bench + docs"`

---

## Execution notes

- **Order:** Tasks 1→11 in codeterm-canvas are sequential-ish (1-3 strictly first); plugin Tasks 12-15 can start after Task 1 lands (they only need the SDK contract); Task 16 last.
- **Async discipline (repo rule):** every plugin call from the daemon stays on the existing poll-loop blocking thread or `spawn_blocking`; never on the Tauri runtime. Observation building snapshots state, then drops locks, then calls the plugin.
- **Capability, not identity (repo rule):** all host checks go through `watcher_capable()` / `spawnable()`; grep yourself for `"lmstudio"` in core before finishing — there must be zero new hits.
- **File sizes:** `spawn_chatbackend.rs` is 16.9K and will grow — if Task 9 pushes it near 1000 lines, split the tick/verdict wiring into `spawn_chatbackend/watcher_hook.rs` rather than allowlisting.
- **Defaults are sacred:** no `mode`/`engine` ⇒ exactly today's behavior; the whole feature is opt-in (spec §Acceptance 5).

## Self-review checklist (run after implementation, before PR)

- [ ] Spec acceptance criteria 1-6 each map to a passing test/bench (1-3 → Task 16, 4 → Task 13f, 5 → Task 13 defaults + existing suite, 6 → Task 3).
- [ ] No `single_message` remnants — the one-shot concept ships as the `machine` engine.
- [ ] `codeterm send` into a watcher rejected at ONE seam (transport), not scattered.
- [ ] Verdict tolerance host-side only; plugin never validates JSON (keeps future plugins thin).

<!-- revision: 2 -->
