# Perf/UX Fix Wave — orchestration plan

> Codex/gpt-5.5 workers, each in its own git worktree off `plugins-rethink`.
> Workers report/push; the orchestrator gates (tsc + pnpm test:pw:lint + pre-push
> clippy) and MERGES every track into `plugins-rethink`. Root fixes only — NO
> band-aids. Measure/spec first. Final verify on Windows/ROG.
> node_modules: each worktree runs its OWN `pnpm install --frozen-lockfile` —
> NEVER symlink node_modules (it corrupted the shared pnpm store once).

## North-star — ONE unified "agent provider plugin" model (capability-differentiated)

There are NOT two worlds (PTY "agent providers" claude/codex/opencode vs
"open-shell" lmstudio/ollama). They are ONE class: **agent provider plugins**.
All of them land in the same agents/providers list, are spawnable as sessions,
and can back ANY agent — including Main / the General Agent (Main already runs on
a provider). They differ ONLY by declared CAPABILITIES:
- transport: PTY vs chatBackend (non-PTY),
- behavior-config model: own-prompt-set / configurable presets+system-prompt /
  graph / none (see spectrum below),
- model-switch, presets, system-prompt, etc.
The current `kind==agent` vs `kind==shell` fork (separate agent_registry /
chatbackend_registry, branching in +new session) is what should CONVERGE onto a
single capability model over time: any provider plugin can back any agent
(lmstudio/ollama/OpenClaw/a future first-party core = peers of claude/codex).
This is the direction; don't deepen the fork — move surfacing/spawn toward
capability checks so the two worlds merge. (Big design goal — informs every track;
the immediate fixes T1-T4 must not add new id/kind hardcoding that fights it.)

### EMERGENCE is the litmus test of the abstraction

A provider plugin, once it declares capabilities, must participate EVERYWHERE an
agent is used — with ZERO per-site code:
- backs **Main / the General Agent** (the always-on top-level orchestrator),
- backs **sub-agents** (spawned workers),
- runs in **crons** (scheduled agents),
- mesh, bridge, anywhere agents run — and the UX.
It "flows in" from the abstraction; it is NOT bolted on the side at each call site.

LITMUS TEST: adding ollama / OpenClaw / a future first-party core must touch ONLY
the plugin (manifest + capabilities) — NOTHING in GA-spawn, cron-spawn,
sub-agent-spawn, or the UX. If it requires touching those sites, **the abstraction
is wrong and the system must be reworked.** Emergence is how we know the design is
right.

CURRENT GAP (honest): lmstudio is NOT emergent today — it spawns via a separate
path (`open_chat_backend_pane`) instead of the agent-provider path
(`build_agent_launch_command`), and GA / cron / sub-agent spawn assume PTY agent
providers. So a chatBackend provider cannot yet back the GA or run in a cron
without special-casing → by the litmus test, the agent-runtime abstraction needs
the unification above. This is the real architectural work behind the surface
fixes; T1-T4 are the visible symptoms, the unified spawn/runtime abstraction is
the cure. Plan a dedicated track for it (T6 — unify agent-provider spawn/runtime
so chatBackend providers are emergent at GA/cron/sub-agent/spawn).

## Design principle — open-shell provider CLASS, capability-driven (governs T1 + all surfacing)

lmstudio is ONE instance of a CLASS: non-PTY chatBackend "open shell" providers
that support system-prompt / presets / model-switch. The class will grow —
ollama, OpenClaw, a future first-party core provider, etc. So every place a
provider surfaces (Plugins panel, "+ new session", three-dots menu, Settings →
providers, the system-prompt/preset config surface, model bubble, …) MUST branch
on CAPABILITY, never on a hardcoded provider id. Examples of the capability
signals to key off: `kind == "shell"` / chatBackend-spawnable, supports
system_prompt, has presets, supports model-switch. NO `if id == "lmstudio"`
anywhere. Adding a new such provider later must require ZERO UI changes — it
surfaces automatically because the UI asks capabilities. Build this generally and
carefully — NO band-aids, no per-provider special-casing.

### Behavior/prompt-config is itself a CAPABILITY SPECTRUM (forward-looking, no band-aids)

Providers in this class differ in WHAT drives their behavior, and the config
surface ("сюда" = the prompt/preset/behavior config) must render per a DECLARED
descriptor, never assume lmstudio's preset model is universal:
- **own-prompt-set** (e.g. OpenClaw) — provider brings its own prompts /
  orchestration (maybe a LangGraph under the hood); CodeTerm offers nothing to
  configure (or only what the provider exposes).
- **configurable presets + system-prompt** (e.g. lmstudio, ollama) — no built-in
  prompts; user supplies system-prompt / prompt presets. (implemented now)
- **graph / other scheme** — future; declared, rendered per its own shape.
- **none**.
So the provider declares its behavior-config model; the config surface adapts.
Goal: bringing OpenClaw / a future first-party provider "into our plugins" needs
ZERO surface special-casing — it declares its model, the UI honors it. Design the
descriptor extensibly now (lmstudio = presets+system-prompt); do NOT hardcode.

OPEN SURFACE: the "сюда" Andrey pointed at = this prompt/behavior config surface
(the 11.10.13 screenshot was lost; confirm exact location). Implement under the
capability-spectrum rule above. This is a DESIGN constraint for the provider work
(T1 + the eventual config-surface track) — not a license to special-case lmstudio.

## Tracks

### T1 — Extensions / Channels UI  *(frontend + light backend; isolated)*
- 1a. Extensions panel does NOT show the list of registered plugin **channels** —
  add a channels-list section (name + url + core/marketplace; add/remove for
  non-protected). Backend already knows them (`codeterm plugin channel list`).
- 1b. Channels/plugins do NOT refresh on entering the Extensions screen or on
  add-channel — add **auto-refresh on panel mount + after add-channel** (trigger
  channel refresh + snapshot rebuild). Today the snapshot only rebuilds on a
  plugin-dir file change — that is the bug.
- 1c. `plugins/_viewtest` removed (dev junk) — keep it gone; fix the
  `pluginCatalog.test.ts` fixture if needed (keep tests green).
- 1d. **Uninstall ghost (was T5):** after uninstalling a plugin (e.g. lmstudio)
  it STILL appears in "+ new session" and the three-dots menu — the provider is
  not dropped live. `deregister_plugin` (hot_reload.rs:121-131) already unregisters
  chatBackend, and the watcher deregisters on "folder removed" (318-332) — so the
  bug is either uninstall not triggering deregister, or the menu
  (`useAgentProviders` -> `list_agent_providers`, Sidebar.tsx:110) not refreshing
  after deregister (same gap as 1b). Fix: uninstall must remove the provider from
  the live menu (deregister fires + menu/provider-list auto-refreshes). Verify:
  uninstall lmstudio -> it disappears from +new session and three-dots WITHOUT
  restart.
- 1e. **Installed plugin provider missing from Settings:** an installed shell/
  chatBackend provider (lmstudio) does NOT show in Settings -> providers
  (AgentsTab.tsx uses the SAME `useAgentProviders` -> `list_agent_providers` as the
  +new session menu). Likely AgentsTab filters to kind=agent and drops kind=shell,
  or the list excludes installed plugin shells. Fix: installed plugin providers
  (incl. shell) appear in Settings -> providers consistently with the +new session
  menu. Verify: install lmstudio -> it shows in Settings providers.
- 1f. **Naming: "Plugins" everywhere, NOT "Extensions".** Standardize all
  user-facing copy on the word "Plugins" (panel title, nav label, headings,
  tooltips, "managed by plugins", etc.). Internal dir/code names may stay; only
  user-facing strings must be consistent on "Plugins".
- Files: `src/components/Extensions/*`, `cli/src/cmd/plugin/channel.rs`,
  `src-tauri/src/commands/plugins.rs`.

### T2 — "Unknown agent provider" startup race  *(backend; isolated)*
- claude/codex/opencode are JS plugins registered ASYNC at startup; right after a
  restart, `+ new session` (build_agent_launch_command / spawn) can fire BEFORE
  they register → "Unknown agent provider".
- Root fix: spawn/open path must not see a half-empty registry — either gate
  agent-spawn until bundled providers are registered, or wait briefly / return a
  clear "providers loading" transient instead of "Unknown agent provider".
- Files: `lib.rs` (startup order), `plugin/js_runtime/loader`, `agent/registry`,
  `multiplexer/commands.rs` (build_agent_launch_command), `service/spawn*`,
  `service/spawn_params.rs` (classify_spawn_target).

### T3 — Tab-OPEN refactor (Problem B: blink / no-tab / double-tab)  *(after T1/T2)*
- Spec the correct behavior FIRST: 1 click = exactly 1 tab, appears immediately;
  repeat click while a spawn is in flight = ignored (no second tab); provider
  error = clean error + 0 tabs.
- Refactor the racy `spawnAgentTab`/`createTab` (~6-10 uncoordinated IPC +
  optimistic dispatch + refreshFromBackend) into ONE atomic backend command that
  creates+configures+activates the pane and returns the snapshot; frontend
  applies it ONCE; in-flight guard on the button.
- Files: `src/store/multiplexer/index.tsx`, `multiplexer/commands.rs`,
  `service/spawn*`.

### T4 — Switching/load lag (Problem A)  *(after T3; frontend)*
- Measure FIRST (instrumentation exists, window.__ctPerf). Find the SYNCHRONOUS
  blocker during lazy-load (chat-ui / PTY mount, worst on first-open-after-restart)
  that freezes the UI. Fix the root.
- Files: `usePtyConnection`, `useChatSession`, `useXterm`, `store/multiplexer/context`.

## Waves (by file-conflict)
- **Wave 1 (parallel): T1 ‖ T2** — disjoint (Extensions/CLI vs startup/registry).
- **Wave 2: T3** — touches index.tsx + multiplexer; spec then code.
- **Wave 3: T4** — touches mount/context, overlaps T3's index.tsx → after T3.
- Order: `{T1 ‖ T2} → T3 → T4`.

## T6 — Unified, emergent agent-provider runtime (the cure)  *(no-legacy rip-and-replace)*

CONTEXT: project start, single user (Andrey), no external base. Migrations may be
done BY HAND per machine. **No backward-compat, no shims, no migration paths —
delete the fork at the root.** We are free to change anything; correctness from a
fresh install is the only bar.

### Target model (ONE of everything)
- **ONE registry.** Merge `agent_registry` + `chatbackend_registry` into a single
  `ProviderRegistry`. No second store, no "is it a shell?" lookup fork.
- **ONE descriptor.** A single `ProviderDescriptor` carrying:
  - `transport: Pty | ChatBackend` (the substrate discriminant),
  - `behavior_config: None | OwnPromptSet | PresetsPlusSystemPrompt | Graph`
    (+ `system_prompt_delivery: CliFlag(flag) | SessionConfig | External`),
  - `models`, `default_model`, `presets`, `model_switch`, `settings_schema`,
    `supports_chat_ui`, `supports_stop`, `preserves_context_on_resume`, trust.
  - **Delete** the hardcoded `kind: "agent"|"shell"` in `list_agent_provider_values`
    (agent_routes.rs) — `kind` is DERIVED from `transport`, nothing branches on a
    literal string. Frontend reads `transport`/capabilities, never `kind`/id.
- **ONE spawn path.** Delete the `open_chat_backend_pane` vs
  `build_agent_launch_command` fork. `spawn_pane` resolves the descriptor and
  drives a single `AgentTransport` seam:
  ```
  trait AgentTransport {              // two impls, picked by descriptor.transport
    fn launch(&self, ctx) -> SessionHandle;   // Pty: spawn+readiness+task ; Chat: openSession
    fn run(&self);                            // Pty: backend_read_loop ; Chat: poll_loop
    // I/O already abstracted by existing PaneTransport (send/kill/resize) — reuse it
  }
  ```
  Pty impl = current PTY lifecycle (readiness strategy, task delivery, read loop).
  Chat impl = current chatbackend openSession + poll loop. Both already exist —
  T6 unifies their *entry points*, it does not rewrite the substrates.

### Emergence (the acceptance test)
The 6 PTY-assuming sites MUST route through the one spawn path / transport seam,
so they stop assuming PTY:
1. `service/general_agent/lifecycle.rs` (GA / Main),
2. `service/orchestrator.rs::spawn_agent` (visible),
3. `service/spawn.rs::spawn_hidden_agent_impl` (hidden / sub-agents),
4. `service/cron.rs` cron-trigger (build `Agent` vs `ChatBackend` by capability, not hardcode),
5. `service/agent_restart.rs` (needs a non-PTY resume path),
6. `service/pty_io.rs::maybe_resume_session` (skip PTY relaunch for chat transport).
LITMUS: after T6, installing ollama/OpenClaw and pointing the GA / a cron / a
sub-agent at it touches ONLY the plugin — zero edits in these 6 sites. If any site
still needs a per-provider branch, T6 is not done.

### Behavior-config spectrum (the "сюда" surface)
The prompt/behavior config surface renders per `descriptor.behavior_config`:
- `OwnPromptSet` (OpenClaw) — provider brings its own prompts; we configure only
  what it exposes.
- `PresetsPlusSystemPrompt` (lmstudio/ollama) — user supplies system-prompt/presets.
- `Graph` — future; rendered per its own shape. `None` — nothing.
Delivery declared (`CliFlag` for PTY agents like claude `--system`; `SessionConfig`
for chatBackend). UI honors the descriptor; no lmstudio special-casing.

### No-legacy deletions (allowed, do them)
- Drop the dual-registry lookups in `service/spawn_params.rs::classify_spawn_target`.
- Drop `kind` string plumbing end-to-end (Rust descriptor + `src/lib/types.ts` +
  `src/lib/paneProvider.ts` + `store/multiplexer/index.tsx:318` branch).
- Collapse `AgentProvider` / `ChatBackendProvider` onto the single descriptor +
  transport seam (one trait, or one builder feeding the registry).
- On-disk pane `backing`/session state: hand-migrate per machine if shapes change.

### Absorbs T2 and T3 (they are symptoms of the fork)
- **T2** ("Unknown agent provider" startup race) is correctly solved when there is
  ONE registry with a known "providers ready" gate on the single spawn path —
  fold T2 into T6 rather than patching the old dual path.
- **T3** (atomic tab-open: 1 click = 1 tab, in-flight guard, apply-once) is the
  frontend half of the ONE spawn path: the unified backend command returns the
  snapshot, the store applies it once. Fold T3's backend command into T6; keep
  T3's frontend in-flight-guard work as the client side of the same seam.
- **T1** (channels/Plugins UI) is disjoint → already merged independently.
- **T4** (switch/load perf) is frontend mount/context → mostly disjoint, after T6.

### Execution order (revised under no-legacy)
1. T1 — DONE (gated + merged into plugins-rethink @ 034a0b41).
2. T2 — DONE as interim (startup readiness gate `require_agent_provider_ready`);
   its gate is absorbed permanently by the unified `ProviderRegistry` in T6.0a.
3. **T6 — full implementation spec: `2026-06-19-t6-unified-provider.md`.** Task split:
   - **T6.0a** descriptor+registry unification (one descriptor w/ `transport`+
     `behavior_config`, `ChatBackendProvider::describe()`, one `ProviderRegistry`,
     rewrite listing/classify, FE drop `kind`→`transport`). Existing spawn paths
     keep working unchanged.
   - **T6.0b** spawn seam `AgentTransport` + atomic/async/**no-blink** open (T3:
     one spawn command returns snapshot, apply-once, generation guard, async launch,
     in-flight guard). Depends on T6.0a.
   - **T6.1–6** emergence migration of the 6 PTY-assuming sites (GA, orchestrator,
     hidden, cron, restart, pty_io resume) → through the seam. Fan-out after T6.0b.
   - **T6.F** behavior-config UI surface renders per `behavior_config`.
4. T4 — perf measure + fix on top of the unified runtime.
5. Final verify on ROG (Windows).

## Discipline
- Each worker: own worktree, own `pnpm install` (no symlink), spec/measure → root
  fix → tsc + pnpm test:pw:lint (+ cargo/clippy for Rust) green → report.
- Orchestrator re-runs the gate itself, reviews diff, merges into plugins-rethink.
- Dev (7686) is a singleton — coordinate runtime between workers.
- Final verify on ROG (Windows, worst case).

<!-- revision: 2 -->
