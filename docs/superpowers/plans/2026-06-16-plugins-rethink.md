# Plugins Rethink Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Generalize "plugin = agent provider" into "plugin = one or more capabilities" (agent / secret backend / transcriber), give the JS runtime the host-API surface (subprocess, network, per-plugin secrets) those plugins need under granted permissions, and ship the Transcriber and Bitwarden plugins plus the Extensions UX.

**Architecture:** Each capability is a core-owned trait (`AgentProvider`, `SecretStore`, new `Transcriber`) dispatched by a dynamic registry the `PluginManager` populates on activate/deactivate. A JS adapter (`JsProvider` exists; add `JsSecretBackend`, `JsTranscriber`) wraps `plugin.js`. Host API bumps v1→v2 with permissioned `exec`/`fetch`/`secret*`. Bitwarden and Transcriber become JS plugins in the `codeterm-plugins` channel; native Bitwarden is deleted after a parity gate.

**Tech Stack:** Rust (Tauri v2, axum, parking_lot, async-trait, QuickJS via the existing `js_runtime`), React/TypeScript (SchemaRenderer, bridge), TS plugins (`plugin.js` + `plugin.json`).

**Spec:** `docs/superpowers/specs/2026-06-16-plugins-rethink-design.md`. Read it first.
**Gap analysis:** `docs/superpowers/plans/2026-06-16-plugins-rethink-gap-analysis.md`.

---

## Gap corrections (apply during execution — from the gap analysis)

- **G1 (Task 1.5/1.6/1.9) — CORRECTED by 2nd pass:** `host.exec`/`host.fetch` are
  **synchronous** host functions: exec via `std::process::Command`, fetch via
  `reqwest::blocking` (the `blocking` feature IS enabled in `Cargo.toml:41`). **Do NOT use
  `spawn_blocking`** — `PluginVm` is not `Send` (rquickjs `Runtime`/`Context`). The async
  adapter trait methods call sync `call_json` **directly on the calling thread, mirroring
  `JsProvider`'s exact mechanism** (read `provider.rs` first and copy it). Brief I/O block
  on that thread is acceptable.
- **G8 (Task 1.4) — NEW from 2nd pass — secret registry factory-vs-instance:** built-in
  backends build from config via `SecretBackendFactory::build(&Value)`; a plugin contributes
  a **ready `JsSecretBackend` instance**. The dynamic registry must serve both:
  `build_active` checks the registry for a ready instance FIRST, else falls back to the
  factory. Change `SecretStoreState` from `Arc<RwLock<Box<dyn SecretStore>>>` to
  `Arc<RwLock<Arc<dyn SecretStore>>>` and have factories + registry both yield
  `Arc<dyn SecretStore>`, so a plugin instance can occupy the active slot without being
  rebuilt. `switch_backend` must not discard a plugin instance.
- **G9 (Task 1.7) — NEW from 2nd pass — `install()` needs `plugin_id`:** thread `plugin_id`
  through `loader.rs::load_one` → `PluginVm::from_source_sandboxed`/`load_with_manifest_sandboxed`
  → `host_api::install(ctx, plugin_id, manifest_json, sandbox)` so `host.secret*` can
  namespace keys as `codeterm.plugin.<id>.<name>`.
- **G2 (Task 1.8):** add `capabilities` + `network`/`subprocess`/`secrets` to the **JS**
  manifest (`js_runtime/manifest.rs`: `PluginManifest`, `PermissionsConfig` — camelCase,
  keep `deny_unknown_fields`). Native `PluginCapabilities` gets `secret_backends`/
  `transcribers` only for symmetry.
- **G3 (Task 1.3):** `format_attachment_line` has **three** call sites — telegram bridge
  (`bridge/manager/mod.rs:569`, has `svc`), chat route (`api/web_routes.rs`, has `AppState`),
  and the Tauri command (`commands/tab.rs::transcribe_voice_attachment(path)`, **no state —
  add `State<'_, …>`**). Thread the `TranscribeRegistry` + active id through all three.
- **G4 (Task 1.4/1.10):** add `transcribe_registry: Arc<TranscribeRegistry>` to
  `ServiceContext`; add `set_transcribe_registry`/`set_secret_registry` to `PluginManager`
  (mirror `set_agent_registry`); wire in `lib.rs`. Prefer passing registries over the manager.
- **G5 (Task 1.2/1.3):** seed the transcribe registry with a built-in `HttpEngine`
  (`builtin-http`) from `VoiceTranscribeConfig`; `transcribe.active` defaults to it →
  zero regression before any plugin install.
- **G6 (Task 1.9):** name parity fixtures `provider-transcriber`/`provider-bitwarden` (the
  loader expects `provider-<id>/plugin.json`); keep the `require_plugins()` skip guard.
- **G7 (Task 1.10):** subscriber loop must survive lag —
  `match sub.recv().await { Ok(ev)=>pm.dispatch_event(&ev), Err(Lagged(_))=>continue, Err(Closed)=>break }`.

---

## File structure

**Phase 1 — Foundation (Rust)**
- `src-tauri/src/plugin/capability.rs` *(new)* — generic `CapabilityRegistry<T>`.
- `src-tauri/src/transcribe/mod.rs` *(new)* — `Transcriber` trait, `Transcript`, `TranscribeError` (moved from `bridge/voice_transcribe.rs`), `TranscriberStatus`, the `transcribe` registry.
- `src-tauri/src/transcribe/http_engine.rs` *(new)* — native HTTP transcriber (the migrated daemon client) used as a parity reference + fallback.
- `src-tauri/src/secrets/registry.rs` *(new)* — dynamic `SecretStore` registry replacing the static factory table.
- `src-tauri/src/plugin/js_runtime/host_api.rs` *(modify)* — add `exec`/`fetch`/`secret*`, bump `HOST_API_VERSION`.
- `src-tauri/src/plugin/js_runtime/host_exec.rs` *(new)* — subprocess host fn + binary resolution (reuse bitwarden detection).
- `src-tauri/src/plugin/js_runtime/host_net.rs` *(new)* — fetch host fn + allowlist.
- `src-tauri/src/plugin/js_runtime/host_secret.rs` *(new)* — per-plugin keychain bucket.
- `src-tauri/src/plugin/js_runtime/manifest.rs` *(modify)* — `capabilities`, extended `permissions`.
- `src-tauri/src/plugin/js_runtime/secret_backend.rs` *(new)* — `JsSecretBackend: SecretStore`.
- `src-tauri/src/plugin/js_runtime/transcriber.rs` *(new)* — `JsTranscriber: Transcriber`.
- `src-tauri/src/plugin/mod.rs` *(modify)* — register/unregister capability adapters on activate/deactivate; `PluginCapabilities` gains `secret_backends`, `transcribers`.
- `src-tauri/src/lib.rs` *(modify)* — event-bus subscriber → `dispatch_event`; build registries into `ServiceContext`.
- `src-tauri/src/bridge/voice_transcribe.rs` *(modify)* — `format_attachment_line` resolves the active `Transcriber` from the registry.
- `src-tauri/src/config/schema/mod.rs` *(modify)* — `integrations.transcribe.active`; keep `secrets.backend`.

**Phase 2 — Transcriber plugin (TS)**
- `plugins/transcriber/plugin.json`, `plugins/transcriber/src/plugin.ts` (+ build to `dist/plugin.js`), `settings.schema.json`, `icon.svg`. (Staged in-repo under `plugins/` for dev, published to the channel repo.)

**Phase 3 — Bitwarden plugin (TS)**
- `plugins/bitwarden/plugin.json`, `plugins/bitwarden/src/*.ts` (port of `secrets/bitwarden/*`), `settings.schema.json`, `icon.svg`.
- Delete `src-tauri/src/secrets/bitwarden/` after parity.

**Phase 4 — Extensions UX (React)**
- `src/components/Extensions/*` *(new)* — top-level view, tray, cards, grant dialog, capability chip.
- `src/components/Titlebar/Titlebar.tsx`, `MeshPresenceTray.tsx` *(modify)* — Extensions entry; machine picker.
- `src/components/Sidebar/Sidebar.tsx` *(modify)* — presence header replaces "Tabs".
- `src/App.tsx` *(modify)* — `mainView` gains `"extensions"`.
- `src/lib/bridge.ts` *(modify)* — `plugin_manager_*` + capability cmds into `HTTP_MAP`.
- `src/components/Settings/{PluginsTab,SecretsTab}.tsx` *(modify)* — promote / re-surface.

---

## Phase 1 — Foundation

### Task 1.1: Generic capability registry

**Files:**
- Create: `src-tauri/src/plugin/capability.rs`
- Test: inline `#[cfg(test)]` in the same file (per CLAUDE.md, move to `capability/tests.rs` if it grows past the limit)
- Modify: `src-tauri/src/plugin/mod.rs` (add `pub mod capability;`)

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    trait Greeter: Send + Sync { fn hi(&self) -> String; }
    struct A; impl Greeter for A { fn hi(&self) -> String { "a".into() } }

    #[test]
    fn register_get_unregister() {
        let r: CapabilityRegistry<dyn Greeter> = CapabilityRegistry::new();
        r.register("a", Arc::new(A));
        assert_eq!(r.get("a").unwrap().hi(), "a");
        assert_eq!(r.active().len(), 1);
        r.unregister("a");
        assert!(r.get("a").is_none());
    }
}
```

- [ ] **Step 2: Run to verify it fails** — `./build.sh check` (compile error: `CapabilityRegistry` undefined). Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
//! Generic runtime registry mapping a capability id to its implementation.
//! Populated by the PluginManager as plugins activate/deactivate. The core
//! always resolves capabilities through a registry, never a hardcoded id list.
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;

pub struct CapabilityRegistry<T: ?Sized> {
    entries: RwLock<HashMap<String, Arc<T>>>,
}
impl<T: ?Sized> CapabilityRegistry<T> {
    pub fn new() -> Self { Self { entries: RwLock::new(HashMap::new()) } }
    pub fn register(&self, id: impl Into<String>, imp: Arc<T>) {
        self.entries.write().insert(id.into(), imp);
    }
    pub fn unregister(&self, id: &str) { self.entries.write().remove(id); }
    pub fn get(&self, id: &str) -> Option<Arc<T>> { self.entries.read().get(id).cloned() }
    pub fn active(&self) -> Vec<Arc<T>> { self.entries.read().values().cloned().collect() }
    pub fn ids(&self) -> Vec<String> { self.entries.read().keys().cloned().collect() }
}
impl<T: ?Sized> Default for CapabilityRegistry<T> { fn default() -> Self { Self::new() } }
```

- [ ] **Step 4: Run** — `cargo test plugin::capability` (via `./build.sh test` or narrowed). Expected: PASS.
- [ ] **Step 5: Commit** — `git add -A && git commit -m "feat(plugin): generic capability registry"`

### Task 1.2: Transcriber trait + module (promote voice_transcribe types)

**Files:**
- Create: `src-tauri/src/transcribe/mod.rs`, `src-tauri/src/transcribe/http_engine.rs`
- Modify: `src-tauri/src/lib.rs` (`pub mod transcribe;`), `src-tauri/src/bridge/voice_transcribe.rs` (re-export moved types or update imports)

- [ ] **Step 1: Write the failing test** (in `transcribe/http_engine.rs` tests) — port the existing `ok_returns_text` / `http_5xx_is_error` / `empty_text_is_error` tests from `voice_transcribe.rs` against `HttpEngine::transcribe`, asserting it implements `Transcriber`.
- [ ] **Step 2: Run** — `./build.sh check`. Expected: FAIL (`Transcriber`, `HttpEngine` undefined).
- [ ] **Step 3: Implement**

```rust
// transcribe/mod.rs
use std::path::Path;
use async_trait::async_trait;

pub mod http_engine;

#[derive(Debug, Clone)]
pub struct Transcript { pub text: String }

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TranscriberStatus {
    Ready, Loading, Unloaded, Unavailable { reason: String },
}

#[derive(Debug)]
pub enum TranscribeError { Disabled, EmptyPath, InvalidUrl(String), HealthFailed(String),
    RequestFailed(String), HttpStatus(u16), ParseError(String), Empty, NoBackend }
// Display impl: move verbatim from voice_transcribe.rs, add NoBackend => "no transcriber installed".

#[async_trait]
pub trait Transcriber: Send + Sync {
    fn id(&self) -> &str;
    async fn status(&self) -> TranscriberStatus;
    async fn transcribe(&self, audio_path: &Path, language: &str) -> Result<Transcript, TranscribeError>;
}

pub type TranscribeRegistry = crate::plugin::capability::CapabilityRegistry<dyn Transcriber>;
```

`http_engine.rs`: move the body of today's `voice_transcribe::transcribe` (health cache, client build, POST `{url}/transcribe`) into `HttpEngine { daemon_url, timeout, ttl, language_default }` implementing `Transcriber`. This is the native parity reference + fallback.

- [ ] **Step 4: Run** — `cargo test transcribe::`. Expected: PASS.
- [ ] **Step 5: Commit** — `feat(transcribe): Transcriber trait + http engine (promoted from voice_transcribe)`

### Task 1.3: Wire the active transcriber into `format_attachment_line`

**Files:** Modify `src-tauri/src/bridge/voice_transcribe.rs`, `src-tauri/src/service/mod.rs` (hold `Arc<TranscribeRegistry>` + active id), `src-tauri/src/config/schema/mod.rs` (`integrations.transcribe.active: String`).

- [ ] **Step 1: Failing test** — `format_attachment_line` with a registry holding a stub `Transcriber` returns `mk::voice(path, Some("hi"), None)`; with an empty registry returns the marker carrying the **install-offer** note.
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement** — change signature to take `&TranscribeRegistry` + active id; resolve `registry.get(active)`; on `None` → note `"install the Transcriber plugin from your channel to transcribe voice notes"`; on `Some` → call `.transcribe()` and map Ok/Err to the existing marker shapes. Update both call sites (chat composer route, telegram bridge) to pass the registry from `ServiceContext`.
- [ ] **Step 4: Run** — `cargo test voice_transcribe`. PASS.
- [ ] **Step 5: Commit** — `feat(transcribe): route voice notes through the active Transcriber capability`

### Task 1.4: Dynamic secret registry (replace static factory)

**Files:** Create `src-tauri/src/secrets/registry.rs`; modify `src-tauri/src/secrets/{mod,factory}.rs`, `src-tauri/src/lib.rs`.

- [ ] **Step 1: Failing test** — register `local`+`mock`; `build_active("mock")` returns the mock; `build_active("nonexistent")` falls back to `local`; runtime `register("bitwarden", …)` then resolves.
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement** — a `SecretStoreRegistry` (reuse `CapabilityRegistry`-style or a purpose struct holding factory closures, since `SecretStore` needs per-backend config at build time). Keep `SecretBackendFactory` trait; move the `OnceLock` table into a runtime registry seeded with `local`/`keychain`/`mock` at startup and extended by the PluginManager. `build_active_store(cfg)` resolves through it; unknown → `local`. Preserve the atomic-swap `Arc<RwLock<Box<dyn SecretStore>>>`.
- [ ] **Step 4: Run** — `cargo test secrets::`. PASS.
- [ ] **Step 5: Commit** — `refactor(secrets): dynamic backend registry (factory table → runtime registry)`

### Task 1.5: Host API v2 — `exec`

**Files:** Create `src-tauri/src/plugin/js_runtime/host_exec.rs`; modify `host_api.rs` (bind fn, bump `HOST_API_VERSION = 2`, add `subprocess` to `PluginSandbox`).

- [ ] **Step 1: Failing test** — with sandbox `subprocess.allow=["echo"]`, `host.exec({bin:"echo",args:["hi"]})` returns `{code:0, stdout:"hi\n"}`; `bin:"bw"` (not allowed) → error.
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement** — `host_exec(ctx, json)` parses `{bin,args,env,stdin,timeoutMs}`, checks `sandbox.can_exec(bin)` (new), resolves bin via reused `which`+platform fallbacks (extract from `bitwarden/subprocess.rs::detection_candidates`), spawns with captured stdio, returns JSON. Secrets via env/stdin only.
- [ ] **Step 4: Run** — `cargo test js_runtime::host` (narrowed). PASS.
- [ ] **Step 5: Commit** — `feat(plugin/host): host.exec under subprocess allowlist (host_api v2)`

### Task 1.6: Host API v2 — `fetch`

**Files:** Create `host_net.rs`; modify `host_api.rs`, `manifest.rs` (`permissions.network.allow`).

- [ ] **Step 1: Failing test** — spawn a local axum server (pattern from `voice_transcribe.rs` tests); `host.fetch({url})` to an allowed host returns status+body; a non-allowlisted host → error.
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement** — `host_fetch(ctx, json)` parses `{url,method,headers,body,timeoutMs}`, checks `sandbox.can_reach(host)` against `network.allow`, uses a shared `reqwest::Client`, returns `{status,headers,body}`. Legacy `network:true` = allow-all + warn.
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** — `feat(plugin/host): host.fetch under network allowlist`

### Task 1.7: Host API v2 — per-plugin secrets

**Files:** Create `host_secret.rs`; modify `host_api.rs`, `secrets/keychain.rs` (namespaced service helper).

- [ ] **Step 1: Failing test** — `host.secretSet("k","v")` then `host.secretGet("k")=="v"`, scoped under `codeterm.plugin.<id>.k`; a different plugin id can't read it; `secretList()` lists only this plugin's keys.
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement** — `host_secret_*` bound only when `permissions.secrets` is granted; key namespace = `codeterm.plugin.{plugin_id}.{name}`; back with `Keychain`.
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** — `feat(plugin/host): per-plugin secret bucket`

### Task 1.8: Manifest — capabilities + extended permissions

**Files:** Modify `src-tauri/src/plugin/js_runtime/manifest.rs`, `src-tauri/src/plugin/mod.rs` (`PluginCapabilities` adds `secret_backends`, `transcribers`).

- [ ] **Step 1: Failing test** — parse a `plugin.json` with `capabilities.transcriber=true`, `permissions.network.allow=[…]`; assert fields populate; `host_api:2` accepted.
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement** — add `capabilities: Capabilities { agent_provider, secret_backend, transcriber: bool }` and `permissions: { network: NetPerm, subprocess: ExecPerm, secrets: bool, filesystem }` to the manifest structs (serde defaults so v1 manifests still parse).
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** — `feat(plugin): manifest capabilities + structured permissions`

### Task 1.9: JS adapters — `JsTranscriber`, `JsSecretBackend`

**Files:** Create `src-tauri/src/plugin/js_runtime/transcriber.rs`, `secret_backend.rs`; modify `loader.rs` (build adapters from declared capabilities), `mod.rs` (export).

- [ ] **Step 1: Failing test** — using the existing `PluginVm` test harness, load a tiny fixture `plugin.js` exporting `transcribe(path,lang){return {text:"x"}}`; build `JsTranscriber`; assert `.transcribe()` returns `"x"`. Same for a fixture exporting `secretGet` → `JsSecretBackend.get`.
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement** — mirror `JsProvider`: each adapter holds the `PluginVm`, calls the exported fn by name, parses JSON into the trait's types, maps errors. Add parity_test cases (`parity_test/transcriber.rs`, `parity_test/secret_backend.rs`) proving JS matches the native `HttpEngine` / a reference on shared fixtures.
- [ ] **Step 4: Run** — `cargo test js_runtime::` PASS; `cargo test parity` PASS.
- [ ] **Step 5: Commit** — `feat(plugin): JsTranscriber + JsSecretBackend adapters + parity`

### Task 1.10: PluginManager registers capabilities; wire event bus

**Files:** Modify `src-tauri/src/plugin/mod.rs` (on activate, build+register declared adapters into the registries held in `ServiceContext`; on deactivate, unregister + fallback), `src-tauri/src/lib.rs` (subscriber loop → `dispatch_event`; construct `TranscribeRegistry` + secret registry into `ServiceContext`).

- [ ] **Step 1: Failing test** — activating a fixture plugin with `capabilities.transcriber` makes `transcribe_registry.get(id)` resolve; deactivating clears it. A published `SystemEvent` reaches a stub plugin's `on_event`.
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement** — registration hooks in `activate`/`deactivate`; in `lib.rs` add `let mut sub = events.subscribe(); tokio::spawn(async move { while let Ok(ev) = sub.recv().await { pm.dispatch_event(&ev); } });`.
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** — `feat(plugin): register capability adapters on activate; wire event bus to dispatch_event`

**Phase 1 gate:** `./build.sh check && ./build.sh test` green; `scripts/check-file-sizes.sh` green.

---

## Phase 2 — Transcriber plugin

### Task 2.1: Plugin scaffold + manifest

**Files:** Create `plugins/transcriber/plugin.json` (`capabilities.transcriber=true`, `permissions.network.allow=["127.0.0.1:7891"]`, `permissions.secrets=true`, `host_api:2`, `settings_schema`), `plugins/transcriber/src/plugin.ts`, `settings.schema.json`, `icon.svg`, build config to emit `dist/plugin.js`.

- [ ] Step 1: Write `settings.schema.json` (`SettingField[]`): `engine` (select: local|mesh-peer), `model` (select), `peer` (select, when engine=mesh-peer), `watchdog_vram_gb` (number, toggle), `watchdog_game` (toggle), `daemon_url` (text, when engine=local).
- [ ] Step 2: Write `plugin.ts` exporting `transcribe(path, lang)` → resolve engine from settings → local: `host.fetch({url:`${daemon}/transcribe`, body:{path,language:lang}})`; mesh: fetch the peer transcribe route. Export `transcribeStatus()`.
- [ ] Step 3: Build → `dist/plugin.js`. Load via `scan_roots` in dev.
- [ ] Step 4: Manual: `./build.sh dev --wait`, set `integrations.transcribe.active="transcriber"`, record a voice note → transcript appears.
- [ ] Step 5: Commit — `feat(plugins): transcriber plugin (local + mesh engine)`

### Task 2.2: Watchdog

- [ ] Step 1: In `plugin.ts`, implement `on_event(ev)` reacting to system/idle events; track load state in `host.secret*`; on GPU pressure (read via `host.exec` of a platform GPU query, allowlisted) unload; reload lazily in `transcribe()`.
- [ ] Step 2: Manual verify load/unload transitions logged.
- [ ] Step 3: Commit — `feat(plugins/transcriber): watchdog load/unload`

---

## Phase 3 — Bitwarden plugin

### Task 3.1: Port `bw` client to TS behind `host.exec`

**Files:** Create `plugins/bitwarden/src/{bw.ts,cipher.ts,store.ts,plugin.ts}`, `plugin.json` (`capabilities.secret_backend=true`, `permissions.subprocess.allow=["bw"]`, `permissions.secrets=true`, `permissions.network.allow=["vault.bitwarden.com"]`), `settings.schema.json`.

- [ ] Step 1: Port `secrets/bitwarden/cipher.rs` envelope parse/build to `cipher.ts` (pure functions; unit-testable in TS).
- [ ] Step 2: Port `secret_store.rs` CRUD to `store.ts` using `host.exec({bin:"bw",args:[…],env:{BW_SESSION}})`; session/master-password via `host.secret*`.
- [ ] Step 3: Export `secretStatus/unlock/lock/logout/save/get/list/search/delete/organizations/collections/sync` for `JsSecretBackend`.
- [ ] Step 4: Commit — `feat(plugins): bitwarden secret-backend plugin (bw via host.exec)`

### Task 3.1b: Issue #176 — kill the keychain legacy (bundled with Bitwarden)
- ✅ DONE (this cycle): `keyring` dep + `secrets-keyring` feature removed from `Cargo.toml`
  (both were dead — file backend is already the default, no boot canary, no OS-keychain code path).
- Remaining: rename the misleading `Keychain` type → `SecretFile` across `secrets/local.rs`,
  `secrets/keychain.rs`→`secret_file.rs`, `plugin/js_runtime/host_secret.rs` (bitwarden/* is
  deleted in 3.2, so no rename needed there). Confirm `~/.codeterm/.secrets` `0600`. Verify a
  signed release launch shows **no** Keychain prompt. No legacy left.

### Task 3.2: Parity gate, then delete native Bitwarden

**Files:** `src-tauri/src/plugin/js_runtime/parity_test/secret_backend.rs`; delete `src-tauri/src/secrets/bitwarden/`; modify `secrets/factory.rs` (drop the static bitwarden entry), `config/schema/mod.rs` (drop `BitwardenSettings` from core; the server URL becomes plugin settings), `SecretsTab.tsx`.

- [ ] Step 1: Parity test — run shared fixtures (mocked `bw` responses) through the ported TS store via the JS adapter; assert identical `SecretRef`/`SecretValue` outputs to the pre-deletion reference.
- [ ] Step 2: Run parity green.
- [ ] Step 3: Delete `secrets/bitwarden/`; fix compile; backend registry now gets `bitwarden` only from the plugin.
- [ ] Step 4: `./build.sh check && test` green; manual: install plugin, unlock, list/get/save; telegram-token fetch still works.
- [ ] Step 5: Commit — `refactor(secrets): move Bitwarden to plugin, delete native backend`

---

## Phase 4 — Extensions UX

### Task 4.1: Capability chip + grant dialog
- [ ] `CapabilityChip.tsx` (granted/denied/system variants, JetBrains Mono, indigo tint) + `InstallGrantDialog.tsx` rendering `manifest.permissions` as a subset-selectable chip list. Match `design-v2` Screen 02. Test: chip renders granted/denied; dialog returns the chosen subset.
- [ ] Commit — `feat(extensions): capability chip + install grant dialog`

### Task 4.2: Extensions tray + top-level view
- [ ] `ExtensionsTray.tsx` (rows, gear-on-hover, pin, groups Pinned/Plugins/Agents/System) using the real `MeshPresenceTray` dropdown chrome (220px, `#1e1e2e`, active tint + ✓). `App.tsx` `mainView` gains `"extensions"`. Titlebar entry replaces/augments the power button (Keep-Awake becomes a `system` plugin row).
- [ ] Commit — `feat(extensions): tray + top-level view`

### Task 4.3: Plugin settings tabls + Settings→Plugins promote
- [ ] Plugin detail uses `SchemaRenderer` over `settings_schema`. `PluginsTab` becomes the VS Code-style marketplace (channels/list/detail/changelog). `SecretsTab` keeps the backend picker (now lists registered backends incl. plugin ones); Bitwarden config moves to its plugin tabl.
- [ ] Commit — `feat(extensions): schema settings + marketplace`

### Task 4.4: Sidebar presence + machine picker + web parity
- [ ] `Sidebar.tsx` "Tabs" → presence header (real dot); web UI adds the machine-picker dropdown beside it. `bridge.ts` adds `plugin_manager_*` + capability/secret commands to `HTTP_MAP`.
- [ ] Commit — `feat(extensions): sidebar presence + machine picker + web parity`

**Phase 4 gate:** `npm run build` (or dev build) green; Playwright smoke of the tray + grant dialog; manual click-through of all design screens.

---

## Testing strategy (maximize coverage; design for testability)

**Unit (per task, TDD — already in force):** every Rust task ships failing-test-first.
Current: capability registry (2), transcribe trait+engine (17 incl. the 8 promoted daemon
tests), secrets dynamic registry (80 incl. the new "instance beats factory" test). Keep this
bar for 1.5–1.10.

**Design for testability (mock seams):**
- `host.exec` goes behind a `HostExec` trait (default = real `std::process::Command`; tests
  inject a fake returning canned `{code,stdout,stderr}`) — mirrors the existing `BwExec`
  seam, so the Bitwarden TS port is testable without a real `bw`.
- `host.fetch` behind a `HostFetch` seam (tests point at a local axum server, the pattern
  `transcribe/http_engine/tests.rs` already uses).
- JS adapters (`JsTranscriber`/`JsSecretBackend`): `parity_test` fixtures under
  `provider-transcriber`/`provider-bitwarden` assert the JS path equals the native reference
  on shared inputs (mocked `host.exec`/`host.fetch`). Guarded by `require_plugins()`.
- Permission gating: explicit unit tests that a plugin WITHOUT `subprocess.allow`/`network.allow`
  is denied (exec/fetch throw), and per-plugin secret isolation (plugin A can't read B's bucket).

**E2E — new eval set `evals/plugins/`** (real daemon, dev on 7686, driven via `dc` lib):
1. install Transcriber from a local channel → activate → record a voice note via the API →
   assert the `-=-codeterm:voice-=-` marker carries the transcript (fake transcribe daemon).
2. install Bitwarden → unlock (mocked `bw`) → save/get/delete a secret → telegram-token fetch
   still works.
3. permission denial: a plugin missing a capability is blocked end-to-end.
Each has a README + an oracle; manual UI-mount steps flagged (matches `evals/` conventions).

**Version / update regression (you asked for this explicitly):** `evals/plugins/update/` —
publish plugin `v0.2.1` to a test channel, install, exercise the capability; publish `v0.2.2`
(bumped bundle, changed behavior marker), run `plugin_manager_action {kind:"update"}`, assert:
(a) the new version activates, (b) the capability still resolves and works, (c) settings/secrets
persist across the update, (d) bundle-hash verification passes, (e) a deliberately-incompatible
`min_codeterm` bump is gated (not installed) rather than breaking the daemon. This proves
"publish new versions, update, nothing falls off."

**Gates:** `./build.sh check` + `./build.sh test` green after every task; `scripts/check-file-sizes.sh`;
`npm run build` for the frontend; `./build.sh e2e` / the eval sets for behavior; Playwright smoke
for the Extensions tray + grant dialog.

## Phase 5 — Test hardening & release dry-run

### Task 5.1: E2E eval set `evals/plugins/` (install → activate → use → uninstall) per capability.
### Task 5.2: Version-update regression `evals/plugins/update/` (publish vN→vN+1, update, re-verify, compat-gate).
### Task 5.3: Playwright smoke — Extensions tray opens, grant dialog renders capability chips, pin toggles.
### Task 5.4: Full-suite green gate — `./build.sh test`, eval sets, frontend build, file-size check; bring dev up on 7686 and walk the spec's acceptance tests (§13).

## Channel repo
- Rename `rollacode/codeterm-thirdparty-channel-demo` → `rollacode/codeterm-plugins`; publish `transcriber` + `bitwarden` bundles. (Outward action — confirm before pushing.) Dev loads from local `plugins/` until then.

## Self-review notes
- Every spec §7/§8/§9 requirement maps to a task. Connectors (§12) intentionally absent.
- Risk: Bitwarden TS port (Task 3.1/3.2) is the highest — gated by parity before native deletion; `local` keychain remains the floor if it regresses.
- Type consistency: `Transcriber`/`Transcript`/`TranscribeError`/`TranscriberStatus`, `CapabilityRegistry`, `JsTranscriber`/`JsSecretBackend`, `host.exec/fetch/secret*`, manifest `capabilities`/`permissions` used consistently across tasks.

<!-- revision: 1 -->
