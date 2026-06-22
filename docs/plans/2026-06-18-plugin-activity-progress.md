# Plugin Activity / Progress — Implementation Plan

> **For workers (Codex):** Each track = ONE worker on its own branch, STRICT TDD
> (red→green→commit; never leave red; write the test if missing). First
> `codeterm mem get <role-skill>` for your role, then READ this whole plan + your
> Track. **You MUST `git add -A && git commit` on your branch BEFORE reporting** —
> uncommitted work is not accepted. Bake the repo gates into your commit. Report
> `DONE <track>: <branch>, <gate output>, <commit sha>` or `BLOCKED <track>: <q>`.
> No backticks in shell strings you pass to other commands. Workers never spawn workers.

**Goal:** Make plugin operations visible — install/uninstall/update AND plugin
self-bootstrap (transcriber downloading ffmpeg/whisper/model) AND agent-driven
config — surface live activity (phase labels + spinner) in the Extensions panel
and a global toast. No more silent spinners / "hung" transcribe.

**Architecture:** Mirror the proven `graph-worker-*` pattern. A `PluginActivity`
registry on `ServiceContext` emits a `plugin-activity` SystemEvent (Tauri emit +
existing SSE `/api/events/stream` for web). A new `host.progress(label, percent?)`
host API lets a plugin report its own steps mid-call. The frontend subscribes and
renders inline (Extensions) + as a toast.

**Decisions:** surfaces = Extensions inline **and** toast. Granularity = phase
labels now (optional `percent` field reserved; live byte-% deferred — needs
streaming exec, out of scope).

## Repos / branches

Mainline = `plugins-rethink` (all prior work is consolidated there). Track
branches fork off `plugins-rethink` and merge **straight back into it** after the
orchestrator re-runs the gate — NO intermediate feature-integration branch (branch
hygiene: one mainline, no juggling). Orchestrator audits `git branch --no-merged
plugins-rethink` before declaring done.
- A — activity registry + event/SSE — codeterm-alpha — `pa-core`
- B — `host.progress` API + SDK — codeterm-alpha — `pa-hostprogress` (dep: A)
- C — install/uninstall/update activity — codeterm-alpha — `pa-lifecycle` (dep: A)
- D — frontend hook + Extensions UI + toast — codeterm-alpha — `pa-frontend` (dep: A)
- E — transcriber emits progress — codeterm-plugins — `pa-transcriber` (dep: B)
- F — live verify (macOS + ROG) — orchestrator

Dep order: **A first**, then **B + C + D + (E after B)** in parallel, then **F**.

**Gates — codeterm-alpha:** `cd src-tauri && cargo fmt && cargo test <module> && cargo clippy --lib -- -D warnings` (NOTE: `--lib` only; `--all-targets` is red on a pre-existing baseline — gate only on no NEW lints in your files) · `./node_modules/.bin/tsc --noEmit` · relevant `pnpm` package test.
**Gates — codeterm-plugins:** `npm run build:all && npm test && npx tsc --noEmit`.

---

## Track A — PluginActivity registry + event + SSE

**Files:**
- Create: `src-tauri/src/plugin/activity.rs` (registry + types)
- Modify: `src-tauri/src/plugin/mod.rs` (expose module)
- Modify: `src-tauri/src/api/events.rs` (add `PluginActivity` SystemEvent variant)
- Modify: `src-tauri/src/service/mod.rs` (or wherever `ServiceContext` is defined) — add `plugin_activity: Arc<PluginActivityRegistry>`
- Test: inline `#[cfg(test)]` in `activity.rs`

**Contract:**
```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginActivity {
    pub id: String,          // activity id (uuid)
    pub plugin_id: String,
    pub kind: String,        // "install" | "uninstall" | "update" | "bootstrap" | "configure"
    pub label: String,       // human phase label, e.g. "Downloading model (466MB)…"
    pub percent: Option<f32>,// 0..100, optional (reserved; usually None for v1)
    pub status: String,      // "running" | "done" | "error"
    pub error: Option<String>,
    pub started_at: u64,     // unix ms — pass in (no Date::now in build scripts; runtime ok here)
    pub updated_at: u64,
}

pub struct PluginActivityRegistry { /* RwLock<HashMap<String, PluginActivity>> + app_handle + events tx */ }
impl PluginActivityRegistry {
    pub fn start(&self, plugin_id: &str, kind: &str, label: &str) -> String; // returns activity id, emits
    pub fn step(&self, id: &str, label: &str, percent: Option<f32>);          // emits
    pub fn finish(&self, id: &str, error: Option<String>);                    // status done/error, emits
}
```
- Emit on every change: `app_handle.emit("plugin-activity", &activity)` AND push to the broadcast `events` channel as `SystemEvent::PluginActivity(activity)` so the SSE web mirror (`web_routes::events_stream`) relays it. Mirror exactly how `graph-worker-progress` / `workers_progress` emits (see `src-tauri/src/api/mem_routes/workers.rs:96`).
- `SystemEvent` add: `PluginActivity { ...fields... }` (it is `#[serde(tag="event", rename_all="snake_case")]` → event name `plugin_activity`; the dedicated Tauri channel uses `"plugin-activity"`).

- [ ] Step 1: failing test — `registry.start()` returns an id and `get(id)` shows status "running" with the label; `finish(id, None)` → "done". Run; FAIL.
- [ ] Step 2: implement registry + types. Run; PASS.
- [ ] Step 3: add the `SystemEvent::PluginActivity` variant; ensure `events.rs` compiles and `type_name()`/serialization covers it (mirror existing variants). `cargo test api::events` or build.
- [ ] Step 4: wire `plugin_activity` Arc into `ServiceContext` construction.
- [ ] Step 5: gate + commit `feat(plugins): PluginActivity registry + plugin-activity event/SSE (Track A)`.

---

## Track B — `host.progress` host API (dep: A)

**Files:**
- Modify: `src-tauri/src/plugin/js_runtime/host_api.rs` (bind `progress`)
- Modify: `src-tauri/src/plugin/js_runtime/host_compute.rs` (impl reaching `ServiceContext.plugin_activity` via the existing `LateRef<ServiceContext>` carrier)
- Modify: `packages/plugin-sdk/src/host.ts` (type)
- Test: in `host_compute.rs` tests / a host_api test

**Contract:** `host.progress(optsJson)` where opts = `{ label: string, percent?: number, done?: boolean, error?: string }`. First call for a plugin's current operation `start`s an activity (kind "bootstrap"); subsequent calls `step`; `done:true` (or `error`) `finish`es it. Keyed per plugin_id (the sandbox knows its own id). Sync, fire-and-forget. Returns `"{}"`.
- It must be SYNC (no Promise) — consistent with the host.* sync contract. The emit is non-blocking.

- [ ] Step 1: failing test — calling the bound `progress` with `{label:"x"}` registers/updates a PluginActivity for the plugin and emits. (Use a test ServiceContext with a capturing events channel.) FAIL.
- [ ] Step 2: implement the bind + impl. PASS.
- [ ] Step 3: SDK type `progress(opts: { label: string; percent?: number; done?: boolean; error?: string }): void`. `tsc --noEmit` in packages/plugin-sdk.
- [ ] Step 4: gate + commit `feat(plugins): host.progress API → plugin-activity (Track B)`.

---

## Track C — install / uninstall / update activity (dep: A)

**Files:**
- Modify: `src-tauri/src/commands/plugins.rs` (`run_action`: wrap each op in a PluginActivity)
- Test: `commands/plugins.rs` tests

**Contract:** In `run_action`, before invoking the CLI subprocess, `plugin_activity.start(plugin_id, kind, "Installing <id>…")`; on success `finish(id, None)`, on error `finish(id, Some(err))`. Coarse (started→done/error) for v1 — the subprocess is opaque. Use the kind matching the action ("install"/"uninstall"/"update"). For channel ops, label accordingly.
- **Agent-driven note:** when an agent runs `codeterm plugin install` via the CLI directly (not `run_action`), there is no daemon event. v1 scope = UI-driven `run_action` emits activity; the plugin's own `host.progress` (Track B/E) covers the bootstrap regardless of trigger. Document this; a CLI→daemon notify is a follow-up (out of scope, note it in the report).

- [ ] Step 1: failing test — `run_action(install)` emits a start activity then a done activity for the plugin id (capture via a test events channel / registry spy). FAIL.
- [ ] Step 2: implement the wrap. PASS.
- [ ] Step 3: gate + commit `feat(plugins): install/uninstall/update emit PluginActivity (Track C)`.

---

## Track D — frontend: hook + Extensions UI + toast (dep: A)

**Files:**
- Create: `src/lib/usePluginActivity.ts` (or a store slice) — subscribe to `plugin-activity` (Tauri `listen` + the SSE path used by `src/lib/webEvents.ts`), keep `Map<pluginId, PluginActivity>`
- Modify: `src/components/Extensions/PluginDetailPanel.tsx` + `ExtensionsView.tsx` — inline progress (label + spinner; show `percent` as a bar when present) per plugin while an activity is `running`
- Modify: toast surface (reuse `src/components/Toast/*` + multiplexer `toasts`) — show a progress toast for running plugin activities, auto-dismiss on `done`, error styling on `error`
- Test: a `usePluginActivity` reducer test (events → map), runnable via tsx like `pluginConfigure.test.ts`

**Contract:** mirror `useGraphWorkers` (src/components/MemoryPanel/useGraphWorkers.ts) for the subscribe/reduce pattern. The hook exposes `activities: Map<string, PluginActivity>` and the components read by plugin id. Toast: one per active plugin op; updates label in place; dismiss on done.

- [ ] Step 1: failing reducer test — feeding start/step/done events yields the right Map state (running then removed/done). FAIL.
- [ ] Step 2: implement hook + subscribe (Tauri + SSE). PASS reducer test; `tsc --noEmit`.
- [ ] Step 3: wire inline progress into Extensions rows/detail + the toast. `tsc --noEmit`.
- [ ] Step 4: gate + commit `feat(plugins): plugin activity UI — Extensions inline + toast (Track D)`.

---

## Track E — transcriber emits progress (dep: B)

**Files:**
- Modify: `codeterm-plugins/transcriber/src/plugin.ts` (call `host.progress` through bootstrap + transcribe)
- Rebuild: `transcriber/plugin.js`
- Modify: `transcriber/plugin.test.cjs` (stub `host.progress`, assert phases)

**Contract:** emit phase labels:
- `ensureFfmpeg` install path → `host.progress({label:"Installing ffmpeg…"})`
- `ensureEngine` install path → `host.progress({label:"Installing whisper.cpp…"})`
- `ensureModel` download → `host.progress({label:"Downloading model ggml-<id> (~<MB>MB)…"})`
- transcribe run → `host.progress({label:"Transcribing…"})`
- on return → `host.progress({label:"Done", done:true})`
- Guard `typeof host.progress === "function"` (older hosts) so the plugin still works without the API.

- [ ] Step 1: failing test — transcribe path calls host.progress with the expected labels in order (stub records calls). FAIL.
- [ ] Step 2: implement + rebuild plugin.js. PASS.
- [ ] Step 3: gate + commit `feat(transcriber): report bootstrap/transcribe progress via host.progress (Track E)`.

---

## Track F — live verify (orchestrator)

- [ ] macOS `./build.sh dev`: from a clean state, install the transcriber from the channel → see install activity (toast + Extensions). Record a voice note → see "Installing ffmpeg…/Downloading model…/Transcribing…" live, then the transcript. Click Configure-with-AI → agent configures → activity visible. Uninstall → activity.
- [ ] Windows `--mesh ROG`: install + bootstrap progress visible; no regression.

---

## Self-review (vs spec)
- Install/uninstall progress → C (UI path) + D (render). ✓
- Plugin self-bootstrap progress → B (API) + E (transcriber) + D (render). ✓
- Agent-driven config visibility → host.progress covers the plugin's own work (B/E); UI-driven via C. CLI-trigger daemon-notify = documented follow-up. ✓
- Both surfaces (Extensions + toast) → D. ✓
- Phase labels (percent reserved) → A model has optional `percent`; v1 uses labels. ✓
- Types consistent: `PluginActivity` (A) is the single shape emitted by B/C and consumed by D; `host.progress` opts map onto registry start/step/finish. ✓

---

## Pre-dispatch review amendments (AUTHORITATIVE — override track text on conflict)

### Track A (registry = STATE ONLY; emit at call sites)
- **(B2)** `PluginActivityRegistry` holds ONLY `RwLock<HashMap<String,PluginActivity>>` — NO `app_handle`, NO events tx (mirror `GraphWorkerRegistry`, src-tauri/src/mem/workers.rs:106). Methods `start/step/finish` mutate state and RETURN the updated `PluginActivity`. They are unit-testable (state transitions) — that is the only buildable test (ServiceContext needs a real AppHandle; there is no test harness — see host_api/tests.rs:101).
- Provide a free fn `pub fn emit_plugin_activity(svc: &ServiceContext, a: &PluginActivity)` that does BOTH emits: `tauri::Emitter::emit(&svc.app_handle, "plugin-activity", a)` AND `svc.events.publish(SystemEvent::PluginActivity(a.clone()))` (copy the pattern at src-tauri/src/mem_routes/workers.rs:116 + mem/workers.rs:405). Callers (B, C) call a registry method then `emit_plugin_activity(svc, &activity)`.
- **(M2)** The ONLY exhaustive match needing a new arm is `type_name()` in `src-tauri/src/api/events.rs:57`. **(M1)** that arm MUST return the literal `"plugin-activity"` (hyphen) — the frontend keys off `type_name()` for BOTH the Tauri channel and the SSE `event_type`; do NOT return the snake_case serde tag. The SSE serializer (web_routes.rs:109) is a catch-all → no edit there. `EventBroadcaster::publish` is generic → no edit.
- `SystemEvent::PluginActivity(PluginActivity)` newtype variant; `PluginActivity` is `#[serde(rename_all="camelCase")]` so TS sees `pluginId/startedAt/updatedAt`.
- Add `plugin_activity: Arc<PluginActivityRegistry>` to `ServiceContext` (service/mod.rs:73) — update EVERY construction site (grep `ServiceContext {`).

### Track B (vm.rs preamble fold; emit via LateRef svc)
- **(M3)** Bind raw `progressRaw(json: String) -> String` in host_compute (NOT `progress`). Fold in `src-tauri/src/plugin/js_runtime/vm.rs` (the preamble, ~:84): `host.progress = function(o){ return host.progressRaw(JSON.stringify(o || {})); };`. Add `vm.rs` to the file list.
- Impl: `progressRaw` parses `{label, percent?, done?, error?}`, reaches `ServiceContext` via the existing `LateRef<ServiceContext>` carrier (same as host.worker/mem), calls the registry (start on first call for this plugin's current op / step / finish) AND `emit_plugin_activity(svc, &a)`. host.progress IS a call site that has `svc` (via LateRef), so it emits directly. Sync, returns `"{}"`.
- SDK `host.ts`: `progress(opts: { label: string; percent?: number; done?: boolean; error?: string }): void` (object-typed; vm.rs stringifies).

### Track C (thread ServiceContext; real kind strings)
- **(B1)** Change `run_action(action) -> Result<Value,String>` to `run_action(action, svc: &ServiceContext)`. Thread it:
  - `plugin_manager_action` (plugins.rs:26): add param `svc: tauri::State<'_, crate::service::ServiceState>` and pass `&svc` (it currently takes only `action`; sibling cmds like `plugin_glance` already take this State).
  - `web_routes::plugin_action` (web_routes.rs:626/632): pass `&state.svc`.
- **(m1)** Use the REAL action kinds from `plugin_action_args` (plugins.rs:530): `install`→start "Installing <id>…", `remove`→kind "uninstall" label "Uninstalling <id>…", `update`/`update-system`→"update", channel ops→"channel". Emit `start` before `run_plugin_cli` (blocking subprocess, opaque → no intermediate frame) and `finish(None|Some(err))` after.

### Track D (unified listen; keyed toast; TS type)
- **(m3)** Do NOT mirror useGraphWorkers' polling. Use the unified `listen` from `src/lib/bridge.ts` (it auto-routes Tauri in desktop, SSE/webEvents in web): a SINGLE `listen("plugin-activity", handler)` covers both. Reducer maps `pluginId → PluginActivity`.
- Add `PluginActivity` TS interface to `src/lib/types.ts` (camelCase fields) next to `GraphWorkerInfo`.
- **(m2)** In-place toast needs a keyed upsert: add an `UPSERT_TOAST` reducer action in `src/store/multiplexer/reducer.ts` (key = pluginId) OR add `key?: string` to `Notification` and make `ADD_NOTIFICATION` replace-if-key-matches; the hook drives one toast per active plugin op, updates label, dismiss on done/error.
- **(m4)** Reducer test must cover BOTH payload shapes: desktop (struct only) and web (struct + extra `event:"plugin_activity"` tag from the SSE serializer). Ignore unknown fields.

### Track E — unchanged (guard `typeof host.progress === "function"`).
### Verify F — confirm one `listen("plugin-activity")` fires in both desktop + web; bootstrap labels live.
