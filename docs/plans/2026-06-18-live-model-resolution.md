# Live Model Resolution — Implementation Plan

> **For workers (Codex):** Each track = ONE worker on its own branch off
> `plugins-rethink`, STRICT TDD (red→green→commit; write the test if missing),
> commit BEFORE reporting. Role hint inline (role skills 404 in mem). Report
> `DONE <track>: <branch>, <gate>, <sha>` or `BLOCKED`. No backticks in shell
> strings. Never spawn agents. Merge target: `plugins-rethink` (one mainline).

**Goal:** Make the provider model list **live and authoritative** — discovered
from each provider at runtime, canonicalized to real ids everywhere — so model
drift (a renamed/removed model) and casing (display-name leaking into a CLI arg,
e.g. `codex --model GPT-5.5` rejected by a ChatGPT account) become impossible by
construction, not patched per-call.

**Root problems this kills (no-legacy, fix-at-source):**
1. **Static catalog = source of truth.** Models are hardcoded in each
   `plugins/provider-*/plugin.json` `models[]`; `codeterm agent models` reads that
   static list → goes stale when the vendor changes models.
2. **Display→launch casing leak.** `detect_session_model` returns the *display*
   name (`GPT-5.5`); it overwrites `agent_session_model`; resume reuses it →
   `build_agent_command` launches `codex --model GPT-5.5` → rejected (case-sensitive
   for ChatGPT-account Codex). Currently self-recovers to `gpt-5.5 medium` (noise +
   "degrades performance").

**Target architecture (the registry is the single source; manifest is seed):**
- A provider capability **`discover_models()`** queries the provider's CURRENTLY
  available models at runtime (the provider is the only authority).
- A **live `ModelRegistry`** on `ServiceContext`, **seeded** from each manifest's
  `models[]`, **overridden** by `discover_models()`, **TTL-cached** per provider.
  Manifest `models[]` becomes seed/fallback, NOT truth.
- **Canonical resolution** at every consumer: `resolve_model(provider, requested)`
  matches by id OR display **case-insensitively** against the live list → returns
  the canonical **id** (+ display). Unknown/stale → fallback to provider default
  (or nearest) + a notice. The launch arg is ALWAYS a canonical id.
- **Spawn-time** resolution (your ask): on every fresh spawn, resolve the model via
  the registry (TTL-cached; lazy refresh; **never block** — probe failure/timeout
  falls back to last-known/seed). Stale model → fallback + notify.
- `detect_session_model` returns the **id**, not display (defense-in-depth with the
  launch-side canonicalization).

**No-block guarantee:** `discover_models()` is bounded (timeout) and cached. A
spawn never hangs or fails because a probe is slow/offline — it uses cached/seed.

## Repos/branches & deps
Mainline `plugins-rethink`. Tracks merge straight in (one mainline; orchestrator
gates each; sequential merges where files overlap).
- **T1** provider `discover_models()` capability (trait + JS hook + native + parity) — `mr-capability`
- **T2** live `ModelRegistry` (seed + TTL + refresh) on ServiceContext — `mr-registry` (dep: T1 types)
- **T3** canonical `resolve_model` + spawn integration + `detect_session_model`→id + launch uses canonical (kills casing at source) — `mr-resolve` (dep: T2)
- **T4** surfaces: `codeterm agent models` + the model picker read the registry; fallback notice — `mr-surfaces` (dep: T2)
- **T5** per-provider probes: implement `discoverModels` in provider-codex/claude/opencode (starts with recon of each probe mechanism) — `mr-probes` (dep: T1)
- **T6** live verify (macOS + ROG) — orchestrator
Order: **T1 → T2 → {T3, T4, T5} → T6.**

**Gates — codeterm-alpha:** `export CARGO_TARGET_DIR=/Users/kandrey/Developer/.psetup-target` then `cd src-tauri && cargo fmt && cargo build && cargo test <module> && cargo clippy --lib -- -D warnings` (clean for YOUR files; ~145 baseline = pre-existing rustc-1.95 noise) · `./node_modules/.bin/tsc --noEmit` for TS · cli: `cd cli && cargo test <module>`.

---

## Track T1 — `discover_models()` capability

**Role:** Rust plugin-runtime + provider-trait engineer (src-tauri/src/agent + plugin/js_runtime) + SDK.

**Files:**
- Modify: `src-tauri/src/agent/provider.rs` (add trait method)
- Modify: `src-tauri/src/plugin/js_runtime/provider.rs` (JS adapter calls the hook)
- Modify: `src-tauri/src/plugin/js_runtime/parity_test.rs` (native+JS parity for the new method)
- Modify: `packages/plugin-sdk/src/*` (SDK type for the optional `discoverModels` export)
- Test: parity_test + a provider unit test

**Contract:** add to the `AgentProvider` trait:
```rust
/// Query the models this provider currently offers, live. None = "can't probe;
/// use the manifest seed." Bounded by the caller's timeout. Returns canonical ids.
async fn discover_models(&self) -> Option<Vec<ModelInfo>>;  // ModelInfo{ id, display_name, default, .. }
```
- Native default impl returns `None` (→ seed fallback). The JS adapter
  (`JsProvider`) calls the plugin's optional `discoverModels()` export if present;
  absent → `None`. Reuse the existing `ModelInfo`/`SerdeModelInfo` shapes.
- **Parity (architecture rule #2):** add a `parity_test.rs` case proving the native
  and JS paths agree on a shared fixture (a stub provider returning a fixed list).

- [ ] Step 1: failing parity/unit test — a stub JS provider exporting `discoverModels` returns the fixture through `JsProvider::discover_models`; a provider WITHOUT the export returns `None`. RED.
- [ ] Step 2: implement trait method + JS adapter + SDK type. GREEN.
- [ ] Step 3: gate + commit `feat(agent): discover_models() provider capability (T1)`.

---

## Track T2 — live `ModelRegistry` (seed + TTL + refresh)

**Role:** Rust services engineer (src-tauri/src/agent + service).

**Files:**
- Create: `src-tauri/src/agent/model_registry.rs`
- Modify: `src-tauri/src/service/mod.rs` (add `models: Arc<ModelRegistry>` to ServiceContext + every construction site)
- Reference: `src-tauri/src/agent/model_catalog.rs` / `models.rs` (current static read)
- Test: inline registry tests

**Contract:**
```rust
struct ProviderModels { models: Vec<ModelInfo>, source: Source /*Seed|Live*/, fetched_at: Instant }
pub struct ModelRegistry { /* RwLock<HashMap<provider_id, ProviderModels>> + TTL */ }
impl ModelRegistry {
    pub fn seed(&self, provider_id: &str, models: Vec<ModelInfo>);   // from manifest at load
    pub fn get(&self, provider_id: &str) -> Vec<ModelInfo>;          // current (live-or-seed) snapshot
    pub fn is_stale(&self, provider_id: &str, ttl: Duration) -> bool;
    pub fn put_live(&self, provider_id: &str, models: Vec<ModelInfo>); // from discover_models
}
```
- Seeded from each plugin manifest's `models[]` when providers load (so it's never
  empty / never blocks). A separate refresh helper (used by T3 + startup + manual)
  calls `provider.discover_models()` with a bounded timeout and `put_live` on
  success; on failure leaves the prior snapshot (seed or last-live).
- TTL default ~15 min (const, documented). State transitions are unit-testable
  (seed→get; put_live overrides; is_stale by injected clock — pass timestamps in,
  do NOT call Instant::now in tests).

- [ ] Step 1: failing test — seed then get returns seed; put_live overrides; is_stale true after TTL with an injected "now". RED.
- [ ] Step 2: implement registry; wire `models: Arc<ModelRegistry>` into ServiceContext (update ALL construction sites — grep `ServiceContext {`); seed it where providers load (loader). GREEN.
- [ ] Step 3: gate + commit `feat(agent): live ModelRegistry (seed + TTL cache) (T2)`.

---

## Track T3 — canonical resolve + spawn integration (kills casing at source)

**Role:** Rust services engineer (src-tauri/src/service + agent).

**Files:**
- Create/Modify: `src-tauri/src/agent/model_registry.rs` (add `resolve_model`)
- Modify: `src-tauri/src/service/spawn.rs` (`resolve_fresh_agent_model` / `validate_agent_model` consult the registry)
- Modify: `src-tauri/src/service/orchestrator.rs` (`build_agent_command` ~:771 — canonicalize before `model_launch_args`; resume path ~:520 too)
- Modify: provider `detect_session_model` so it returns the canonical id (find where each provider returns display — `js_runtime/provider.rs`, `agent_claude/provider/detection.rs`)
- Test: resolve tests (id match, display-case-insensitive match, stale→fallback) + a spawn-path test

**Contract:**
```rust
pub struct Resolved { pub id: String, pub display: String, pub fell_back: bool }
/// Match `requested` (id OR display, case-insensitive) against the live registry
/// snapshot for `provider`. Unknown/stale → provider default (or nearest by family)
/// with fell_back=true. Result `id` is always what reaches the CLI.
pub fn resolve_model(reg: &ModelRegistry, provider: &str, requested: Option<&str>) -> Resolved;
```
- **Spawn-time (your ask):** `resolve_fresh_agent_model` triggers a lazy TTL refresh
  of the registry for that provider (bounded; non-blocking fallback), then
  `resolve_model` → canonical id. A stale/renamed requested model → fallback id +
  surface `fell_back` (T4 shows a notice).
- **build_agent_command:** run the model through `resolve_model` (or trust the
  already-resolved spawn value) so `model_launch_args` ALWAYS gets a canonical id —
  never a display name. The resume path (orchestrator:520) resolves
  `agent_session_model` the same way.
- **detect_session_model returns id:** fix each provider's detection to map its
  detected display→id (via the registry) before storing in `agent_session_model`.
  Defense-in-depth: even if a display slips in, build_agent_command canonicalizes.

- [ ] Step 1: failing tests — resolve_model: exact id; "GPT-5.5"/"gpt-5.5" both → id "gpt-5.5"; unknown → default with fell_back=true. RED.
- [ ] Step 2: implement resolve_model; wire into spawn + build_agent_command + resume; make detect_session_model return id. GREEN. Add a test asserting build_agent_command never emits a non-id model.
- [ ] Step 3: gate + commit `fix(agent): canonical model resolution at spawn/launch — no display leak (T3)`.

---

## Track T4 — surfaces (CLI + picker read the registry; fallback notice)

**Role:** Rust CLI + React/TS engineer.

**Files:**
- Modify: the `agent models` CLI command (find it — `cli/src/cmd/...`) to read the registry snapshot (via an API/command) instead of the static manifest read
- Modify: the model picker UI (Settings / agent spawn UI) to show the live list
- Modify: surface a `fell_back` notice (reuse the toast / notification system) when a spawn's requested model was substituted
- Test: a snapshot/format test

**Contract:** `codeterm agent models <provider>` shows the live registry list
(falling back to seed); the picker shows the same; when a spawn falls back, the
user sees a one-line notice "model X unavailable → using Y".

- [ ] Step 1: failing test — `agent models` output reflects a registry put_live (not the static manifest). RED.
- [ ] Step 2: implement (registry read path + picker + notice). GREEN; tsc.
- [ ] Step 3: gate + commit `feat(agent): live model list in CLI + picker + fallback notice (T4)`.

---

## Track T5 — per-provider probes (recon-first)

**Role:** Provider-integration engineer (plugins/provider-*).

**Files:**
- Modify: `plugins/provider-codex/plugin.js` (+ claude/opencode as feasible)
- Test: plugin-side tests with a stubbed probe

**Contract:** implement the optional `discoverModels()` export per provider:
- **codex (first — it's the one erroring):** RECON how codex exposes its current
  supported set for the active account (a CLI subcommand / `~/.codex` config / an
  endpoint). Return canonical ids + display. If no clean probe exists, return the
  manifest seed unchanged (still correct) and document the limitation.
- **claude / opencode:** best-effort (Anthropic `/v1/models` if a key/path is
  available; else seed). Each documents its probe + fallback.
- Bounded + cheap; the host caches (T2).

- [ ] Step 1: recon each provider's probe; write it up in the plugin README/comment.
- [ ] Step 2: failing test (stubbed probe returns a list → discoverModels surfaces it). RED → implement → GREEN. Rebuild plugin.js.
- [ ] Step 3: gate + commit `feat(provider): discoverModels probes (codex first) (T5)`.

---

## Track T6 — live verify (orchestrator)
- [ ] macOS `./build.sh dev`: spawn a codex worker → **no `GPT-5.5` error**, launches `gpt-5.5` (canonical). Set a bogus/old model → spawn falls back to default with a notice. `codeterm agent models codex` reflects live (kill network → still returns seed, no hang). Resume a codex pane → still canonical (no display leak).
- [ ] Windows `--mesh ROG`: same, cross-platform.

## Self-review / no-legacy checklist
- Static `models[]` demoted to seed/fallback — registry is the source. ✓
- Casing fixed AT SOURCE (detect→id + launch canonicalizes), not a per-call patch. ✓
- Spawn-time live resolution + fallback (your design) with TTL cache + never-block. ✓
- Capability hook keeps "core asks capability, not identity" + parity test. ✓
- `ModelInfo` shape reused across T1–T4 (no parallel types). Registry holds state
  only; probes/timeouts at the service boundary (testable units, no AppHandle in tests).
