# Plugins Rethink — Gap Analysis

**Method:** cross-reference the plan/spec (desired) against the current codebase
(actual), both directions; categorize each as MISSING / PARTIAL / INCONSISTENT /
CONFIRMED; score confidence; prioritize; recommend. Verified by reading the real code
(two parallel verification passes).

**Overall recommendation:** **MODERATE_GAPS** — the core abstraction (capability trait +
dynamic registry + JS adapter) is **confirmed sound** against the code (agent registry is
already dynamic; `PluginManager` already holds optional registry refs; `SecretBackendFactory`
already takes config at build; `Keychain`/subprocess detection are reusable; the secret
consumer is backend-agnostic). Seven threading/async/targeting gaps must be closed in the
plan before coding. Confidence: high (every item quoted from source).

---

## Confirmed (no change needed)

| # | Assumption | Evidence | Confidence |
|---|---|---|---|
| C1 | JsProvider construction pattern to mirror | `loader.rs:93-127`, `provider.rs:151-179` (`new_with_trust(manifest, Arc<vm>, trust)`) | 100 |
| C2 | Agent registry is dynamic, populated on activate | `registry.rs:64-87`, `mod.rs:321-358` | 100 |
| C3 | PluginManager holds `Option<Arc<AgentRegistry>>` + `events`, with `set_agent_registry` | `mod.rs:207-231` | 100 |
| C4 | Host fn binding + per-VM `PluginSandbox` (`can_read/can_write/can_env`) | `host_api.rs:54-151`, `loader.rs:122-127` | 100 |
| C5 | `SecretBackendFactory::build(&Value)` takes config at build; atomic swap `Arc<RwLock<Box<dyn SecretStore>>>` | `factory.rs:8-62`, `secrets/api.rs:442` | 100 |
| C6 | `Keychain` accepts arbitrary service/key (namespacing safe) | `keychain.rs:55-232` | 100 |
| C7 | Telegram secret consumer is backend-agnostic (`.read().await.has/get`) | `bridge_routes.rs:14-174` | 100 |
| C8 | Bitwarden bin detection is self-contained, extractable | `bitwarden/subprocess.rs:27-93` | 100 |
| C9 | Parity harness pattern (`load_js`, `pair()`, `require_plugins()` guard) | `parity_test.rs:42-77` | 95 |

## Gaps to close (plan corrections)

### G1 — CRITICAL — QuickJS plugin methods are SYNCHRONOUS; `host.exec`/`host.fetch` must be sync-blocking
- **Category:** INCONSISTENT. **Confidence:** 100.
- **Evidence:** `vm.rs:314-330` `call_json` is a **sync free fn**; `vm.rs:372-374` **rejects any returned Promise** ("plugin methods must be synchronous"). The existing host API is all sync-blocking (`readFile`, `sqliteQueryAllJson`).
- **Impact:** The plan's Transcriber (`host.fetch`) and Bitwarden (`host.exec`) are inherently I/O. They CANNOT be async in the VM. So `host.exec`/`host.fetch` must be **synchronous host functions that block the VM thread** until the subprocess/HTTP completes, then return the result. The async `Transcriber`/`SecretStore` trait methods on `JsTranscriber`/`JsSecretBackend` must invoke the sync `call_json` via `tokio::task::spawn_blocking` (PluginVm must be `Send`; confirm — `Arc<PluginVm>` is shared in `JsProvider`, so yes).
- **Closes in:** spec §4 (state exec/fetch are sync-blocking), plan Tasks 1.5, 1.6 (implement as blocking; fetch uses `reqwest::blocking` or a dedicated current-thread runtime `block_on`), 1.9 (adapters wrap `call_json` in `spawn_blocking`).

### G2 — HIGH — manifest target is the JS `PermissionsConfig` (camelCase, `deny_unknown_fields`) + add `capabilities` to the JS manifest
- **Category:** INCONSISTENT. **Confidence:** 100.
- **Evidence:** JS manifest perms `manifest.rs:360-367` = `PermissionsConfig { transport_events, send_text }` with `#[serde(deny_unknown_fields, rename_all="camelCase")]`. Native `PluginPermissions {filesystem, network:bool, mesh_sync}` (`mod.rs:66-75`) is a *different* struct. The plan vaguely said "manifest"; these plugins are JS, so the **JS** `PermissionsConfig` is what gains `network`/`subprocess`/`secrets`, and the **JS** `PluginManifest` gains `capabilities`. (`deny_unknown_fields` is fine — we're adding the fields to the struct.)
- **Closes in:** plan Task 1.8 — target `js_runtime/manifest.rs` `PermissionsConfig` + `PluginManifest`; camelCase serde; keep native `PluginCapabilities` additions only for symmetry/native plugins.

### G3 — HIGH — `format_attachment_line` has THREE call sites, one a stateless Tauri command
- **Category:** PARTIAL (plan assumed two). **Confidence:** 100.
- **Evidence:** (a) telegram bridge `bridge/manager/mod.rs:569-580` — `ServiceContext svc` in scope; (b) chat/API `api/web_routes.rs` — has `AppState`; (c) Tauri IPC `commands/pane.rs::transcribe_voice_attachment(path: String)` — **no state param**. (b) and (c) load `VoiceTranscribeConfig` via `crate::config::load_config()`.
- **Impact:** Routing through the registry requires the registry reachable at all three. The Tauri command needs a `State<'_, …>` added; the web route uses `AppState`; the bridge uses `svc`.
- **Closes in:** plan Task 1.3 — enumerate all three; add state to the Tauri command; thread `&TranscribeRegistry` (+ active id) through each.

### G4 — HIGH — `ServiceContext` lacks `plugin_manager`/`transcribe_registry`; `PluginManager` lacks transcribe/secret registry setters
- **Category:** MISSING. **Confidence:** 100.
- **Evidence:** `ServiceContext` (`service/mod.rs:69-122`) has `events`, `secrets`, **no** `plugin_manager`, **no** transcribe registry. `PluginManager` has only `agent_registry`+`events` (`mod.rs:207-217`).
- **Closes in:** plan Tasks 1.4/1.10 + file structure — add `transcribe_registry: Arc<TranscribeRegistry>` (and the dynamic secret registry handle) to `ServiceContext`; add `set_transcribe_registry`/`set_secret_registry` to `PluginManager` (mirror `set_agent_registry`); wire in `lib.rs` before/at `ServiceContext` construction. Add a `plugin_manager: Arc<PluginManager>` field to `ServiceContext` if a call site needs it (the registries themselves may suffice — prefer passing registries, not the manager).

### G5 — MEDIUM — zero-regression migration: register the native HTTP engine as a built-in transcriber
- **Category:** PARTIAL (plan risked a behavior gap before plugin install). **Confidence:** 95.
- **Evidence:** today voice notes work via `VoiceTranscribeConfig.daemon_url`. If `active` defaults to none, voice transcription breaks until the plugin is installed.
- **Closes in:** spec §7.3 + plan Task 1.2/1.3 — seed the `transcribe` registry at startup with a built-in `HttpEngine` (id `builtin-http`) constructed from `VoiceTranscribeConfig`; `integrations.transcribe.active` defaults to `builtin-http` when `voice_transcribe.enabled`. Installing the Transcriber plugin lets the user switch `active` to it. No regression, and the plugin path is exercised by choice.

### G6 — LOW — parity loader expects `provider-<id>` dir naming + `require_plugins()` skip guard
- **Category:** INCONSISTENT. **Confidence:** 90.
- **Evidence:** `parity_test.rs:42` loads `workspace_plugins_root()/provider-<id>/plugin.json`; tests skip when `require_plugins().is_none()`.
- **Closes in:** plan Task 1.9 — either name capability-plugin fixtures `provider-transcriber`/`provider-bitwarden`, or generalize the loader to take a dir name. Keep the `require_plugins()` skip guard so CI without bundled plugins stays green.

### G7 — LOW — broadcast `recv()` errors on lag; subscriber loop must not exit
- **Category:** MISSING (robustness). **Confidence:** 100.
- **Evidence:** `events.rs:93-120` uses `tokio::sync::broadcast` (cap 256); `recv()` returns `Err(Lagged)`/`Err(Closed)`.
- **Closes in:** plan Task 1.10 — loop `match sub.recv().await { Ok(ev)=>pm.dispatch_event(&ev), Err(Lagged(_))=>continue, Err(Closed)=>break }`.

## Note (no change)
- N1 — `PluginSandbox::can_env` denies names containing TOKEN/KEY/SECRET/PASSWORD/CREDENTIAL/AUTH/SESSION (`host_api.rs`). Bitwarden passes `BW_SESSION` through `host.exec`'s own `env` param (not `host.envGet`), so it is unaffected. The exec `env` param must be allowed to carry such names (it is the plugin's own subprocess env, gated by the `subprocess` permission).

## Coverage check (spec → task, post-fix)
- §2 capability model → Tasks 1.1–1.2, 1.4, 1.9, 1.10 ✓
- §4 host API v2 (sync) → Tasks 1.5–1.7 (G1 applied) ✓
- §5 manifest → Task 1.8 (G2 applied) ✓
- §6 lifecycle + event bus → Task 1.10 (G4, G7 applied) ✓
- §7 Transcriber (+ builtin-http) → Tasks 1.2/1.3 (G3, G5 applied), Phase 2 ✓
- §8 Bitwarden → Tasks 1.4 (G4), Phase 3 ✓
- §9 UX → Phase 4 ✓

---

## Second pass (after Tasks 1.1–1.2 implemented)

Re-ran adversarially against the corrected plan + new code. Two **critical** corrections to
the first pass, plus one new threading detail:

- **G1 was half-wrong → corrected.** `spawn_blocking` is impossible: rquickjs
  `Runtime`/`Context` are not `Send`, `PluginVm` mutex-guards them. Adapters call sync
  `call_json` **directly on the calling thread, mirroring `JsProvider`**. `host.exec`
  (`std::process::Command`) / `host.fetch` (`reqwest::blocking`, feature already on) block
  that thread briefly — acceptable. (Spec §4 + plan G1 updated.)
- **G8 — NEW, HIGH.** Secret registry must serve both config-built factories (local/keychain/
  mock) and plugin-contributed ready instances (`JsSecretBackend`). `build_active` prefers a
  registered instance, falls back to factory; `SecretStoreState` becomes
  `Arc<RwLock<Arc<dyn SecretStore>>>`; both factory and registry yield `Arc<dyn SecretStore>`.
  (Plan G8 added.)
- **G9 — NEW, MEDIUM.** `host_api::install` lacks `plugin_id`; thread it through
  `loader`→`vm`→`install` for the `codeterm.plugin.<id>.<name>` secret namespace. (Plan G9 added.)
- Confirmed-OK: reqwest `blocking` feature already enabled (`Cargo.toml:41`); manifest
  optional fields safe with `#[serde(default)]`; 3 transcribe call sites confirmed
  (`commands/pane.rs:416` stateless → add `State<'_, ServiceState>`; `web_routes.rs:593` has
  `AppState`; bridge has `svc`).

**Verdict:** plan is now correct against the code. No remaining architectural blocker. Proceed.

## Third pass (found during Phase 2 implementation — caught by tests)

- **G1 amendment — `reqwest::blocking` PANICS inside a tokio runtime.** The 2nd-pass
  reassurance ("reqwest::blocking uses its own thread, safe from async") was WRONG: building
  a blocking client on a tokio worker panics. Fix shipped in `host_net::run`: perform the
  request on a **dedicated `std::thread`** (`spawn(...).join()`), isolating it from the
  runtime. `host.exec` (std::process) is unaffected. Tests that run an in-process fake daemon
  must use `#[tokio::test(flavor="multi_thread")]` (single worker deadlocks on the join while
  the daemon needs a worker to respond); production daemons are separate processes, so single
  worker is fine there. Verified by `shipped_transcriber_transcribes_via_fake_daemon`.

## Fourth pass — risk register for remaining phases (mitigations)

Scanned the shipped code + the unbuilt phases for what could break or leak. Live risks
verified and cleared; forward risks get an explicit mitigation baked into the plan.

**Cleared (verified safe):**
- R-A `secrets-keyring` removal breaks release/CI — **CLEAR**: no reference in `build.sh`,
  `build.ps1`, `.github/`, `scripts/`, `.codeterm/`; `keyring` gone from `Cargo.lock`.
- R-B default secret backend unusable after Bitwarden→plugin — **CLEAR**: default is `local`
  (file store), needs no plugin; native `bitwarden` factory still registered as the
  parity reference + fallback; a plugin instance wins via `build_active_store` registry-first.

**Bitwarden TS port (Phase 3b) — security-critical; mitigations are mandatory:**
- R-1 credential leakage in logs → secrets travel only via `host.exec` `env`/`stdin` (enforced;
  never argv); `plugin.js` must never `console.log` a value; no secret in `SecretRef`.
- R-2 vault lockout if the port has a bug and native is already deleted → **parity gate**:
  keep native `secrets/bitwarden/` until `parity_test/secret_backend.rs` is green on shared
  fixtures (mocked `bw`); even after deletion, `local` file store remains the floor; default
  stays `local` so a broken plugin never blocks startup.
- R-3 parity drift → the parity test runs both the native reference and the JS backend through
  the same fixtures (mocked `host.exec` responses) and asserts identical `SecretRef`/`SecretValue`.
- R-4 `bw` interactive prompts hang the subprocess → always `--nointeraction --response`, session
  via `BW_SESSION` env (mirrors the native impl exactly).
- R-5 host.exec needs structured `bw` output → already returns `{code,stdout,stderr}`; the TS
  `cipher.ts` parses the same JSON envelope the Rust `cipher.rs` does.

**Known design gaps (forward; chosen approach):**
- G-10 **JS-plugin event delivery**: `dispatch_event` reaches only native `PluginManager.entries`;
  JS plugins (transcriber/bitwarden) are loaded into the capability registries, not the manager,
  so their `on_event` never fires. *Impact:* a JS watchdog can't be event-driven yet. *Mitigation:*
  the transcriber watchdog is **poll-based** (lazy unload-check on each transcribe + optional timer),
  not event-driven; the native event path stays correct and is exercised by tests. Full
  event→VM delivery (call a JS `onEvent` export from the subscriber loop, collecting loaded VMs)
  is a clean follow-up — not a regression, since no shipped feature depends on it.
- G-11 **plugin settings delivery**: `plugin.js` reads config via env (`CODETERM_TRANSCRIBE_URL`)
  rather than CodeTerm's settings store. *Mitigation/plan:* Phase 4 adds a `config.plugins.<id>`
  namespace written by the SchemaRenderer and delivered to the VM (extend `host.manifestJson`
  with a settings blob, or a `host.config(key)` host fn). Until then env works for dev/testing.

**React UX (Phase 4) — de-risking:**
- R-6 can't validate UI headlessly → build the **backend data wiring first** (`bridge.ts`
  `HTTP_MAP` for `plugin_manager_*` + capability/secret commands — pure-logic, testable), then
  components, then a Playwright smoke of the tray + grant dialog. Prioritize grant dialog + tray
  (highest value); marketplace polish last.

<!-- revision: 4 -->

<!-- revision: 2 -->
