# T6 — Unified, emergent agent-provider runtime (implementation spec)

> No-legacy rip-and-replace. Project start, single user, hand-migration allowed.
> Delete the fork at the root. Companion to `2026-06-19-perf-ux-fixwave.md` (T6 section).
> T6 ABSORBS T2 (startup race — already centralized in `require_agent_provider_ready`)
> and T3 (atomic tab-open — the client half of the one spawn command).

## The fork today (3 seams to collapse)
1. **Two traits / two registries.** `AgentProvider` (PTY) + `agent_registry` vs
   `ChatBackendProvider` (`src-tauri/src/chatbackend/mod.rs:57`) +
   `ChatBackendRegistry = CapabilityRegistry<dyn ChatBackendProvider>` (mod.rs:152).
2. **`kind` hardcoded in listing.** `list_agent_provider_values`
   (`src-tauri/src/api/agent_routes.rs:10-68`) serializes agent `ProviderDescriptor`s
   and bolts `kind:"agent"`, then HAND-ROLLS a parallel `kind:"shell"` JSON for
   spawnable shells (lines 40-65) with extra fields (`presets`) not in the descriptor.
3. **Boolean spawn fork.** `classify_spawn_target`
   (`src-tauri/src/service/spawn_params.rs:171-183`) → `Ok(true)` routes to the
   ChatBackend pane path, `Ok(false)` to the PTY agent path. Downstream: the
   `open_chat_backend_pane` vs `build_agent_launch_command` split.

## Target model (ONE of everything)

### 1. Descriptor — extend the single `ProviderDescriptor` (provider.rs:444)
Add, and make it THE only descriptor both transports emit:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderTransport { Pty, ChatBackend }   // replaces the `kind` string

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "model", rename_all = "snake_case")]
pub enum BehaviorConfig {
    None,                        // nothing to configure
    OwnPromptSet,                // provider brings its own prompts (OpenClaw)
    PresetsAndSystemPrompt,      // user supplies system-prompt + presets (lmstudio/ollama)
    Graph,                       // future; rendered per its own shape
}
```
On `ProviderDescriptor` add:
- `pub transport: ProviderTransport,`
- `pub behavior_config: BehaviorConfig,` (default `None`)
- `pub presets: Vec<PresetInfo>,` (promote to first-class; agents default empty)
- `pub system_prompt_delivery: SystemPromptDelivery` — `CliFlag(String) | SessionConfig | External` (declares HOW system-prompt reaches the provider).
- Bump `PROVIDER_DESCRIPTOR_SCHEMA_VERSION`.
- **Remove** the `kind` string entirely (it never was a struct field — it was
  injected in the listing; just stop injecting it). `transport` is the discriminant.

### 2. ChatBackendProvider gains `describe()`
Add `fn describe(&self) -> ProviderDescriptor` to `ChatBackendProvider`, returning a
descriptor with `transport: ChatBackend`, `behavior_config: PresetsAndSystemPrompt`
(default; a backend may override), `presets` from `list_presets()`, `available_models`
from `list_models()`, `supports_chat_ui: true`, `system_prompt_delivery: SessionConfig`,
icons from the existing getters. This DELETES the hand-rolled JSON in agent_routes.

### 3. ONE `ProviderRegistry`
Introduce `src-tauri/src/provider/registry.rs` (or fold into `agent::registry`) — a
single registry holding a unified entry:
```rust
pub enum Provider {
    Pty(Arc<dyn AgentProvider>),
    Chat(Arc<dyn ChatBackendProvider>),
}
```
Exposes:
- `describe_all() -> Vec<ProviderDescriptor>` (each transport-tagged; spawnable filter
  for chat applied here),
- `resolve(id) -> Option<&Provider>`,
- `transport_of(id) -> Option<ProviderTransport>`,
- the **startup-readiness gate from T2** (`wait_for_provider_startup` /
  `ProviderStartupStatus`) covering ALL providers — this is where T2 lands permanently.
Physically: the two existing stores may remain as backing maps inside this one
registry, but **every call-site stops touching two registries** — they go through
`ProviderRegistry`. No `chatbackend_registry` references leak into routes/spawn.

### 4. Rewrite the 3 seam call-sites on the unified surface
- `list_agent_provider_values` → `registry.describe_all()` (one expression; NO `kind`,
  NO hand-rolled JSON).
- `classify_spawn_target` → `match registry.transport_of(id) { Some(ChatBackend)=>…,
  Some(Pty)=>…, None=> readiness-gate Err }` (keep the not-spawnable-shell clean error).
- `available_agent_or_shell_ids` → `registry.describe_all().map(id)` (or a registry
  `ids()`); drop the dual-registry plumbing.

### 5. Spawn seam — `AgentTransport` (the runtime half) + atomic/async/no-blink open (T3)
Introduce `trait AgentTransport { fn launch(&self, ctx) -> SessionHandle; fn run(&self); }`
with two impls wrapping EXISTING code (do NOT rewrite the substrates):
- `PtyTransport` = current `build_agent_launch_command` + PTY lifecycle
  (readiness/task-delivery/`backend_read_loop`).
- `ChatTransport` = current `spawn_chatbackend_impl` (openSession + poll loop).
The single `spawn_pane` dispatch resolves `registry.transport_of()` → picks the impl.
I/O stays on the EXISTING `PaneTransport` seam (send/kill/resize) — reuse as-is.
This deletes the `open_chat_backend_pane` vs `build_agent_launch_command` fork at the
spawn entry.

#### T3 — the OPEN must be atomic, async, and never blink (confirmed live bug)
Symptom (merged tip, observed): click "+ new session" → tab flashes for a few ms,
**disappears**, then 3-5s later reappears. Root causes, both here:

1. **Snapshot race overwrites the new tab.** Every spawn ends with
   `refreshFromBackend()` = a SEPARATE `invoke("mux_get_state")`
   (`src/store/multiplexer/index.tsx:221-229`, called at 250/312/323/345), AND the
   `mux-state-changed` listener fires its OWN `mux_get_state`. These race: a snapshot
   captured BEFORE the pane existed can resolve AFTER one that had it →
   `SET_STATE` clobbers the tab away. No generation guard → an older snapshot
   regresses a newer one. The reappear = the late snapshot that finally includes it.
   - **Fix A (apply-once, authoritative):** ONE atomic backend spawn command
     (`mux_spawn_pane`/equivalent) that creates + cwd + agent_type + activates +
     focuses the pane in ONE call and RETURNS the resulting `MultiplexerState`
     snapshot. Frontend dispatches `SET_STATE` from THAT return value exactly once.
     Remove the per-spawn `refreshFromBackend()` and the chain of separate invokes
     (`mux_set_active_workspace` + `mux_create_tab` + `mux_update_pane_cwd` +
     `mux_set_pane_agent_type` + `mux_set_chat_view` + `mux_set_active_tab` +
     `mux_set_focused_pane`) — they collapse into the one command.
   - **Fix B (monotonic state, kills the whole class):** stamp every `MultiplexerState`
     with a backend-incremented `generation: u64`; the reducer's `SET_STATE` IGNORES a
     snapshot whose `generation` is ≤ the currently applied one. A late/stale
     `mux_get_state` can no longer overwrite a newer state. This protects the
     `mux-state-changed` path too (the merged-tip A4 goal: embed the snapshot in the
     event payload so there is no second fetch at all — do that here).

2. **Spawn blocks the UI for 3-5s.** For chatBackend, `spawn_chatbackend_impl`
   (`src-tauri/src/service/spawn_chatbackend.rs:152`) AWAITS `chatbackend_open_session`
   — a synchronous network call (LM Studio at the configured baseUrl) — before
   returning. The tab cannot settle until it returns.
   - **Fix C (async launch):** the atomic spawn command creates the pane + returns the
     snapshot IMMEDIATELY (pane shows instantly in a "connecting"/pending state);
     `AgentTransport::launch` (openSession for chat, PTY spawn+readiness for pty) runs
     on a background task. Pane visibility NEVER waits on launch. This is the
     emergence-friendly shape: every transport launches async behind the same seam.

3. **Client in-flight guard (FE half of T3):** a per-button in-flight flag so a second
   click while a spawn is pending is ignored (no duplicate tab). The atomic command +
   apply-once makes "1 click = exactly 1 tab"; the guard prevents double-fire.

Tab visibility is MONOTONIC: once a spawn adds a tab, nothing removes it until explicit
close. No optimistic-then-overwrite.

### 6. Frontend — branch on capability, never `kind`/id
- `src/lib/types.ts` — `ProviderDescriptor`: drop `kind`, add `transport`,
  `behavior_config`, `presets`, `system_prompt_delivery`.
- `src/lib/paneProvider.ts` — resolve from `transport`, not `kind`.
- `src/store/multiplexer/index.tsx:318` — replace
  `getAgentDescriptor(agentId)?.kind === "shell"` with
  `…?.transport === "chat_backend"`.
- Grep the whole `src/` for `kind === "shell"` / `"agent"` provider checks and the
  config surface; render the behavior-config surface from `behavior_config`.

### 7. Deletions (no-legacy — do them)
- The `kind` injection + hand-rolled shell JSON (agent_routes.rs:14-66).
- Any `chatbackend_registry` reference outside `ProviderRegistry` internals.
- The frontend `kind`-based branches.
- On-disk pane `backing`/session shapes: if changed, note the hand-migration; dev
  reinstall of lmstudio is acceptable.

## Scope boundary
**T6.0 (this worker) = §1–§6 + §7 foundation + spawn seam.** It must NOT regress the
existing GA/cron/restart/resume sites — they keep working through `PtyTransport`
unchanged. The 6-site emergence migration (GA, orchestrator, hidden, cron, restart,
pty_io resume → route through the seam so a ChatBackend provider can back them) is the
NEXT wave (T6.1–6), fanned out after the foundation lands.

## Acceptance (worker self-gate, then orchestrator re-gate)
- `pnpm exec tsc --noEmit` clean; `pnpm test:pw:lint` clean.
- `./build.sh check` + pre-push clippy (src-tauri + CLI, `-D warnings` + CLIPPY_ALLOW) clean.
- `./build.sh test` green.
- Runtime on dev :7686: `GET /api/agents/providers` returns descriptors with
  `transport` (no `kind`); claude/codex/opencode = `pty`, lmstudio = `chat_backend`.
- lmstudio still spawns a working chat pane (now via the unified spawn → `ChatTransport`).
- NO new `if id == "lmstudio"` / `kind ==` literals anywhere.
- **T3 no-blink (manual + scriptable):** clicking "+ new session" (claude AND lmstudio)
  → the tab appears ONCE and STAYS (no flash/disappear/reappear), even though
  launch/openSession is still completing in the background. Tab is visible
  instantly, not after 3-5s. A second click while pending does NOT create a 2nd tab.
- A stale `mux_get_state` cannot remove a just-created tab (generation guard test).

<!-- revision: 1 -->
