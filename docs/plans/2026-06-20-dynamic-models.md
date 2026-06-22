# Dynamic model lists — wire the discovery that already exists

> Owner: Dante (orchestrator). From Andrey 2026-06-20: model lists are hardcoded +
> go stale (claude showed Opus 4.7; opencode ships `claude-opus-4-7`). Make them
> auto-update. Branch `plugins-rethink`.

## Root cause (not "hardcoded by design")
The dynamic seam is fully built but **never invoked**:
- Every provider already exposes `discoverModels()` (live source):
  claude → `models.ts` anthropic `/v1/models`; codex → `plugin.ts:85`; cursor →
  `cursor-agent --list-models` (`plugin.ts:76`); opencode → `plugin.ts:85`;
  lmstudio → LM Studio `/api/v1/models`.
- Core has `model_catalog::refresh_live(provider)` (calls `discover_models()`,
  writes a cache file) + `available_models()` (descriptor → UI: ChatComposer,
  ChatModelBubble, PresetManager).
- **GAP: `refresh_live` has NO callers.** So the cache is never populated from live;
  `available_models()` returns the static manifest list → stale forever.
- Today only cursor + lmstudio LOOK dynamic because their flows fetch live at use;
  claude/codex/opencode render the manifest curated list untouched.

## Design — wire it, schedule it, merge cleanly, never block

### 1. Wire `refresh_live` (the core fix)
Call it so the cache actually fills from live. Triggers (all async / off hot-path):
- **Startup**: one background refresh per installed provider after boot (spawn_blocking;
  `discover_models` may run sync JS/CLI — never gate startup or the runtime).
- **On model-picker open**: TTL-gated refresh (skip if cache fresh < TTL, e.g. 6h);
  reuse the ExtensionsView TTL+force-refresh pattern. + a manual refresh affordance.
- **Periodic**: optional low-freq background refresh (e.g. every 12–24h) while running.
- Results persist to the `model_catalog` cache file (instant load next boot; refresh
  updates it). `available_models()` reads cache-or-manifest.

### 2. Merge policy — dynamic freshness WITHOUT losing clean UX
> Andrey CONFIRMED: keep the clean curated aliases (`opus/sonnet/haiku/fable`). The
> alias SET stays curated; what's DYNAMIC per alias = (a) version label, (b)
> availability/enabled state. E.g. `fable` is currently DISABLED and must show as such
> (and auto-appear/enable when Anthropic exposes it for the account).
- **Alias providers (claude):** keep stable alias ids `opus/sonnet/haiku/fable` (the CLI
  resolves them to latest) — add the `fable` alias now (missing today). For EACH alias,
  resolve against the live `/v1/models` list:
  - **version label** auto-derived from live (so "opus" always shows the current number —
    no manual bumps; "fable" shows its version when available).
  - **availability/enabled state** from live: if the alias has no matching available model
    for the account (e.g. fable today) → mark DISABLED (greyed, non-selectable) or hide
    per a config knob; when it becomes available in live → auto-enable. So the picker
    truthfully reflects what the account can actually use, tracked automatically.
  - Keep curated descriptions. A `ModelInfo.available`/`disabled` flag carries the state to the UI.
- **Full-id providers (codex/cursor/opencode/lmstudio):** live list is the source of
  truth; curated manifest is the **offline/fallback** list. Filter junk (deprecated /
  embedding / non-chat models). Preserve a sensible `defaultModel` (resolve if the
  hardcoded default disappears from live — fixes opencode's stale `claude-opus-4-7`).
- Always: discover fails / offline → fall back to the curated manifest (never empty).

### 3. Hot-path & boundary discipline
- UI reads the cached `available_models()` INSTANTLY; refresh runs in the background
  (spawn_blocking), then pushes an update event → picker re-renders live.
- Core calls the generic `discoverModels` capability only — no per-provider id branching.
- Native+JS parity for `discover_models` (parity case already exists; extend per provider).

## Tasks (decomposable to workers)
- **DM1 (core, Rust):** wire `model_catalog::refresh_live` — startup async refresh per
  provider + a `refresh_models(provider, force)` entry; cache file with `fetched_at` +
  TTL; `available_models()` returns cache∪manifest (cache wins, manifest fallback).
  Emit a `models-updated` event for the UI. Tests: refresh writes cache; TTL skips;
  offline → manifest fallback; cache survives restart.
- **DM2 (UI seam, TS):** model-picker (ChatComposer / ChatModelBubble) triggers a
  TTL-gated refresh on open + a manual refresh; subscribe to `models-updated` so the
  list updates live. Tests.
- **DM3 (merge/label policy, core):** alias-version enrichment (claude opus→live
  version in label) + full-id live-vs-curated merge + junk filter + default-resolve.
  Tests incl. "stale manifest label corrected by live discovery".
- **DM4 (per-provider discover audit, plugins):** verify/fix each `discoverModels`
  returns current, clean, chat-only models (claude/codex/cursor/opencode/lmstudio);
  fix opencode's stale manifest defaults (`claude-opus-4-7` + defaultModel); confirm
  offline fallback. Rebuild bundles. Parity per provider.
- **DM5 (tests/gates):** end-to-end — a provider with a stale manifest shows the live
  version after refresh; picker reflects it; no hot-path block (check-blocking-in-async);
  cargo + tsc + test:unit + test:plugins + parity green.

## Sequencing
DM1 (core wiring) first — it unblocks everything. DM3 (merge policy) + DM4 (per-provider
audit) after DM1. DM2 (UI) after DM1's `models-updated` event. DM5 throughout.
Gate every merge with the pre-push hook. Cross-platform: ROG verifies on Windows
(cursor-agent / lmstudio live paths differ by OS).

<!-- revision: 1 -->

## DM5 — bring codex/cursor/opencode to claude-parity (Andrey 2026-06-20)
DM1 wired refresh_live for ALL providers + DM4 confirmed codex/cursor/opencode discoverModels
return LIVE data (codex slug cache, cursor `--list-models`, opencode rows) — so their LISTS already
auto-update. MISSING vs claude: clean live labels + availability flags. DM5 closes that:
- codex: ensure the live gpt-5.x list drives the dropdown with clean labels (current versions auto-appear);
  manifest = offline fallback.
- cursor: `--list-models` already strips status; add availability (a model the plan/account can't use ->
  available:false, greyed, like fable) if the live output signals it; clean labels; `auto` stays default.
- opencode: full provider/model ids -> clean display labels; availability for federated providers not
  configured (-> disabled) if detectable; live list source-of-truth, manifest fallback, default-resolve (4-8 done).
- These are FULL-ID providers (no alias-mapping like claude) — apply the DM3 merge policy (live truth +
  manifest fallback + junk filter + availability) + clean displayNames; do NOT invent aliases.
- Tests per provider: discover -> clean current models; availability flag; offline -> manifest fallback.
- Rebuild bundles. MERGE AFTER comment-trim (both touch provider-*/plugin.ts; comment-trim is comment-only).
