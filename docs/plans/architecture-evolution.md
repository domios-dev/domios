# Architecture Evolution Plan

**Status:** Chunk B DONE as of 2026-04-08. Next: Chunk C (Phase B3 dispatch batches).
**Branch:** `feat/memory` (plugin-prep work was consolidated here)
**Goal:** Evolve codebase toward Hexagonal + Command Bus architecture, covered by tests, ready for plugin system.

## Status as of 2026-04-08

- **Phase A** — test foundation: not tracked here; see test coverage reports.
- **Phase B1** — unified dispatch infrastructure: **DONE** (`2a5ef09`). `src-tauri/src/dispatch/mod.rs` has `Command` enum + `dispatch()`.
- **Phase B2** — first 5 ops across REST/Socket/Tauri: **DONE** (`cac954`, `65ce514`, `a8a7f4e`). Health, pane.list, send_text, get_output, get_pane_screen[_formatted].
- **Phase B3** — remaining dispatch ops: **IN PROGRESS**. Pane ops batch 1 (`PaneSplit`/`PaneClose`/`PaneFocus`/`PaneRename`) through REST + Socket (`36b5065`). **Remaining: workspace, pty, agent, mesh ops — this is Chunk C.**
- **Phase C** — EventBus: **DONE** structurally (`88c7afe`). Typed `SystemEvent` bus in `api/events.rs`; `ContributionsChanged { kind }` added in Chunk B.3 (`543bdc6`). Migration of output-pipeline notifications and agent status events to typed variants still outstanding (deferred to Chunk C).
- **Phase D** — Plugin trait + first plugin: **SKELETON DONE** (`f371699`). `Plugin` trait, `PluginManager`, manifest parser in `src-tauri/src/plugin/mod.rs`. `AgentProvider` trait expanded with `auto_approve_flags`, `compact_command`, `model_switch_command`, `exit_command`, `install_skills`, `uninstall_skills`, `supports_lazy_skills`.
- **Phase D.1 + D.2 — Extraction + leak fix**: **DONE** (`8826b98`, 2026-04-07).
  - `agent/claude.rs` → `plugin/agent_claude/provider.rs`; `agent/codex.rs` → `plugin/agent_codex/provider.rs`.
  - `src-tauri/src/agent/` contains ZERO provider-specific code — only generic `provider.rs` trait, `registry.rs`, `detector.rs`, `prompt.rs`, `status.rs`, `reports.rs`, `git_info.rs`.
  - New helper `crate::config::default_agent_provider()` — single source of truth replaces 6 hardcoded `"claude"` defaults.
  - `detector.rs` dead code purged (278 LOC: `AgentDetector`, `DetectedEvent`, `AgentType { ClaudeCode, Aider, Generic }`, helpers). Claude-specific fingerprints moved into the plugin.
  - TODO at `api/routes.rs:1036` — Claude hook handler still literal `"claude"`; proper fix requires Chunk D (plugins can register HTTP routes).
- **Phase D.3 — Chunk B: Provider self-presentation**: **DONE** (2026-04-07..08). See Chunk B section below for the full commit chain.
- Supporting refactors: ANSI helpers extracted to `pty::ansi` (`4cbfd77`); `get_pane_screen` unified through backend VT100 path (`a7c2a5a`).

## Chunk B — DONE (commits `8826b98..3d0d772`, 2026-04-07..08)

Full commit chain on `feat/memory`:

| Commit | Step | Summary |
|---|---|---|
| `8826b98` | D.1 + D.2 | Extract claude/codex providers into plugins; kill 6 hardcoded `"claude"` defaults; scrub 278 LOC dead code from detector |
| `ade0f42` | plan | Flesh out Chunk B design (reviewed with code-reviewer subagent) |
| `dc94506` | B.1 + B.2 | `ProviderDescriptor` wire format (schema_version=1, deny_unknown_fields), 11-variant `SettingField` enum, trait metadata methods, Claude/Codex implement full `settings_schema()` with 100% fidelity to the legacy React panels |
| `543bdc6` | B.3 | `dispatch::Command::ListAgentProviders` + REST `GET /api/agents/providers` + Socket `agents.list_providers` + Tauri `list_agent_providers` (first real parity-tested agent-op through unified dispatch). `ContributionsChanged { kind }` event + bus→Tauri bridge in `lib.rs`. |
| `7609485` | B.4 + B.5 | Spawn validation against registry (reject unknown provider ids). `provider_name == "claude"` literal in `service/general_agent.rs` replaced by new trait method `preserves_context_on_resume()`. Per-provider config `[agents.<id>]` in schema with back-compat fallback to global `[agent]`. First read site migrated (`commands.rs:143`). |
| `6c7b9a2` | B.6 | Frontend: TS types sync (`ProviderDescriptor`, `SettingField` discriminated union), `useContributions<T>` primitive, `useAgentProviders()` wrapper, `<SchemaRenderer />` (~330 LOC, one sub-component per `SettingField` variant). **Deleted** `ClaudeAgentSettings` + `CodexAgentSettings` + all hardcoded provider branches from `SettingsPanel.tsx`. |
| `ff88d24` | plan | Memory plan supplements — 6 skill-sync invariants locked in (rules≠skills, lookup hierarchy, bridge-rule generator, canonical use case, sync_watcher revert as Phase 3.0 prereq) |
| `c7d03e4` | Step 1 | Notifications via provider contribution. `notifications_schema()` trait method + ClaudeProvider impl that reproduces legacy `ClaudeHooksGroup` as a declarative `Section` (Info + PathLink + Badge + Action). **Deleted** `ClaudeHooksGroup` React component; new `ProviderNotifications` iterates `useAgentProviders().filter(p => p.notifications_schema.length > 0)`. |
| `9538e98` | Step 2 | Global Skills section in Settings → Agents tab (hardcoded, CodeTerm-wide, not a provider contribution). **Deleted** `StepSetup` from onboarding (skill+hooks+rules rows); TOTAL_STEPS 5→4. |
| `cc2f8a2` | fix | GA prompt: explicit "save to MEMORY.md, never to Claude auto-memory" section after the Rollo bug |
| `a13fa35` | feature | GA auto-init kick on fresh sessions (backend sends `init` after agent detection when `!use_resume`; prompt teaches init = read MEMORY.md → ls → mesh peers → one-line ack) |
| `1998e47` | chore | `general_agent.md` compressed ~31% (−67 lines / ~−800 tokens) — validated via prompt-evaluator A/B: 0 regressions, +2 strict improvements |
| `3d0d772` | cleanup | Omnibus: personal info scrub (Rollo/ainyiva/BLACKSTAR/MacMini/186809246 → generic), screenshot feature purge (`screenshot.rs` + `tests/screenshot.ps1` deleted, `/api/screenshot` UI reference removed), new `.claude/skills/prompt-evaluator/` skill (maintainer prompt A/B harness), GA prompt refinements (HTML/append Telegram, stronger recon-before-action), eval set refinements (#1/#3/#5/#8 reworded) |

### Acceptance criteria met

- ✅ `SettingsPanel.tsx` contains **zero** `ClaudeAgentSettings` / `CodexAgentSettings` / `ClaudeHooksGroup` / `if (activeProvider === "claude")` branches
- ✅ `src/lib/agentProviders.ts` no longer needed for Settings rendering — `useAgentProviders()` hook feeds everything
- ✅ `GET /api/agents/providers` returns `Vec<ProviderDescriptor>` identical across REST / Socket / Tauri transports (parity-tested via the same `dispatch::Command::ListAgentProviders` path)
- ✅ `POST /api/agents/spawn` rejects unknown provider ids with `Err("Unknown agent provider: 'X'. Available: [...]")` before touching workspaces / PTYs
- ✅ `src-tauri/src/agent/` contains zero provider-specific code — only generic primitives
- ✅ A hypothetical third native plugin (`plugin/agent_aider/`) would appear automatically in Settings / Sidebar / CommandPalette after a restart with **zero** frontend changes
- ✅ Rust tests: **399 passed** (was 385 before Chunk B), 0 failed. 0 TypeScript errors.
- ✅ Prompt compression validated by `prompt-evaluator` harness A/B (no regressions, +2 improvements)
- ✅ Personal info (real names, machine names, Telegram IDs) scrubbed from all shipped artifacts

### Deferred out of Chunk B

- **Phase 3.0 sync_watcher revert** — memory plan prerequisite, stays untouched per the explicit hands-off rule below
- **Plugin HTTP route contribution** — required to move `api/routes.rs:1036` Claude hook handler and `commands.rs` Claude installer code into the plugin. Chunk D territory.
- **`provider_name == "claude"` → `preserves_context_on_resume()` migration is done**, but other provider-specific special cases in `commands.rs:675-700` and `commands.rs:857-900` (Claude rules installer, Claude hooks installer) still live in core. Chunk D.
- **Generic `detect_agent_session_id(provider_id, ...)`** instead of `detect_claude_session_id` wrapper at `multiplexer/commands.rs:197` — Chunk D (backward-compat rename).
- **Output-pipeline and agent-status events** still emit raw Tauri events instead of typed `SystemEvent` variants — Chunk C or later.

## Chunk B design reference (historical)

Kept below for future contribution-type work (skills, commands, panels will reuse the same patterns). If you're adding a new plugin contribution kind, read B.1-B.7 as the template.

### Design decision (reviewed 2026-04-07)

Two designs were considered: (A) one-liner `list_descriptors()` over the existing provider HashMap, vs (B) separate descriptor cache + generalized `ContributionRegistry` covering skills/commands/panels/etc. Consensus after review: **build Design A. Add three forward-compat hooks that cost nothing now and make the eventual contribution-registry refactor a one-day job, not a rename of half the project.**

Rationale:
- Design A fully delivers everything in B.1-B.7. The cache in Design B is not *needed* by any current requirement — it's a bet on contribution kinds that don't exist in code yet.
- "ProviderDescriptor and SkillDescriptor share `id` + `display_name` and nothing else" — a unified container is just a namespace of unrelated maps, not a real abstraction. Don't commit to that namespace until two populated maps actually exist.
- YAGNI applies to *implementation*. *Naming/shape* should be forward-compatible so the second contribution type is additive, not retrofit.

**Forward-compat hooks (apply to B.1, B.3, B.6):**
1. **Lifecycle lives on `PluginManager`, not on `AgentRegistry`.** When a plugin activates, `PluginManager.activate()` calls `plugin.agent_providers()` and registers them. When contribution #2 lands, adding it = `+1` line in the same method (`plugin.skills()` → push into a new sibling map). No retrofit of existing call sites.
2. **Generic event name.** Emit `contributions-changed { kind: "agent_provider" }` (NOT `provider-descriptor-changed`) from `PluginManager` on attach/detach. Frontend filters by `kind`. Skills/commands reuse the same channel.
3. **Frontend hook parameterized internally.** `useContributions<T>(kind, fetcher)` is the primitive; `useAgentProviders()` is a one-line wrapper (`useContributions<ProviderDescriptor>("agent_provider", fetchProviders)`). Zero cost, makes the second hook trivial.

**Wire-format insurance:** `ProviderDescriptor` carries `schema_version: u32 = 1` and `#[serde(deny_unknown_fields)]`. Round-trip golden test before exposing the endpoint. Cheap now, no clients to break, hard to add later.

**Concurrency contract:** `AgentProvider::describe()` MUST be pure, cheap, no I/O, no locks, no `.await`. Document in the trait. `list_descriptors()` collects into a `Vec` and drops the read guard before serializing. The descriptor cache (Design B) does NOT make this safer — it just hides stale data instead of surfacing it.

**Explicitly NOT in Chunk B:** `ContributionRegistry` umbrella struct, descriptor cache / `update_descriptor()` / push invalidation, `SkillDescriptor`/`CommandDescriptor`/etc, generic `ContributionKind` trait. These land in Chunk B+1 *only when the second contribution type has a real implementation behind it*. Trigger: when adding contribution #2 requires touching `PluginManager.activate()` for the second time, refactor that method to iterate `Vec<Box<dyn ContributionKind>>`. Not before.

### B.1 — Trait extension (backend, no behavior change)

- [ ] Define new types in `src-tauri/src/agent/provider.rs`:
  - `pub struct ModelInfo { id: String, display_name: String, description: Option<String> }`
  - `pub struct ProviderDescriptor { schema_version: u32, id, display_name, icon_hint, available_models, default_model, settings_schema, supports_lazy_skills, has_hooks }` — the wire format. `schema_version = 1` from day one. `#[serde(deny_unknown_fields)]` + serialize/deserialize round-trip golden test.
- [ ] **Rich `SettingField` enum** — must cover everything currently rendered by `ClaudeAgentSettings` and `CodexAgentSettings` in `src/components/Settings/SettingsPanel.tsx:820-980` (sections, toggles, conditional warnings, info cards, path links, status badges, multi-state action buttons). Variants:
  ```rust
  pub enum SettingField {
      // Grouping
      Section { title: String, description: Option<String>, fields: Vec<SettingField> },

      // Inputs
      Toggle { key: String, label: String, default: bool, description: Option<String> },
      Text   { key: String, label: String, default: Option<String>, placeholder: Option<String> },
      Number { key: String, label: String, default: Option<i64>, min: Option<i64>, max: Option<i64> },
      Select { key: String, label: String, options: Vec<(String, String)>, default: Option<String> },
      ApiKey { env_var: String, label: String, link: Option<String> },

      // Display
      Info     { markdown: String },                            // formatted text with inline `code`
      PathLink { label: String, path: String, open: PathOpenKind }, // OpenFolder | OpenFile | RevealInExplorer
      Badge    { query_command: String, variants: Vec<BadgeVariant> }, // queries backend, colored badge

      // Actions
      Action {
          id: String,
          label: String,
          command: String,           // Tauri command name to invoke
          state_query: Option<String>, // optional command returning {variant, label, style} for dynamic state
          confirm: Option<String>,
          style: ActionStyle,        // Primary | Secondary | Danger
      },

      // Conditional visibility
      When { field: String, equals: serde_json::Value, then: Vec<SettingField> },
  }
  ```
  Plus support enums: `BadgeVariant { id, label, style }`, `BadgeStyle { Green | Orange | Red | Gray }`, `ActionStyle`, `PathOpenKind`.
- [ ] Add methods to `AgentProvider` trait with empty default impls (so nothing breaks):
  - `fn available_models(&self) -> Vec<ModelInfo> { vec![] }`
  - `fn default_model(&self) -> Option<&str> { None }`
  - `fn icon_hint(&self) -> Option<&str> { None }`
  - `fn settings_schema(&self) -> Vec<SettingField> { vec![] }`
  - `fn preserves_context_on_resume(&self) -> bool { true }` — replaces the `provider_name == "claude"` literal at `service/general_agent.rs:410`.
  - `fn describe(&self) -> ProviderDescriptor` — default impl assembles from the methods above.
- [ ] **Document the `describe()` contract in the trait doc-comment:** "MUST be pure, cheap, no I/O, no locks, no `.await`. Called while holding a read lock on the registry."

**Schema completeness rule:** if Chunk B uncovers a UI pattern in the existing Claude/Codex panels that doesn't fit the variants above, **add a new variant to the enum**. Do NOT add an "escape hatch" component slot. There is no legacy mode and no hardcoded fallback — the schema is the only path. We have no external users to protect; if the schema needs to grow, grow it.

### B.2 — ClaudeProvider / CodexProvider implement full metadata

- [ ] `plugin/agent_claude/provider.rs`:
  - `available_models` — sonnet, opus, haiku (current Claude Code ids)
  - `default_model` = sonnet, `icon_hint`, `preserves_context_on_resume = true`
  - **`settings_schema` must reproduce 100% of `ClaudeAgentSettings` from `SettingsPanel.tsx:820-931`:**
    - Section "Session Resume" with two `Toggle`s + `When` block showing an `Info` warning when `resume_skip_permissions = true`
    - Section "Rule" with `Info` description, `PathLink` to `~/.claude/rules/codeterm.md`, `Badge` driven by `check_rule_installed`, `Action` "Install" wired to `install_project_rules` with `state_query = check_rule_installed`
- [ ] `plugin/agent_codex/provider.rs`:
  - `available_models` (gpt-5, o1, etc.), `default_model`, `icon_hint`, `preserves_context_on_resume = false`
  - **`settings_schema` reproduces 100% of `CodexAgentSettings` from `SettingsPanel.tsx:933-980`:**
    - Section "Session Resume" with two `Toggle`s + conditional warning
    - Section "Requirements" with `Info` containing the install instructions (markdown with inline code)
- [ ] Tests in each provider's `mod tests` covering the metadata + a snapshot test asserting the schema serializes to a stable JSON shape.

### B.3 — Expose via unified dispatch (first real agent-op Phase B win)
- [ ] Add `AgentRegistry::list_descriptors()` — one-liner over the existing `RwLock<HashMap>`:
  ```rust
  pub fn list_descriptors(&self) -> Vec<ProviderDescriptor> {
      let guard = self.providers.read();
      let out: Vec<_> = guard.values().map(|p| p.describe()).collect();
      drop(guard);
      out
  }
  ```
  No second cache, no parallel storage. Same `providers: RwLock<HashMap<String, Box<dyn AgentProvider>>>` we have today.
- [ ] New command `dispatch::Command::ListAgentProviders` in `src-tauri/src/dispatch/mod.rs` returning `Vec<ProviderDescriptor>` from `registry.list_descriptors()`.
- [ ] Wire through three adapters via existing dispatch glue:
  - REST: `GET /api/agents/providers`
  - Socket: `"agents.list_providers"` JSON-RPC method
  - Tauri: `#[tauri::command] list_agent_providers`
- [ ] Parity test: same input → identical JSON across all three transports.
- [ ] **PluginManager emits `contributions-changed { kind: "agent_provider" }`** (generic event name, not provider-specific) on `attach_native` / `detach`. Both via the typed `SystemEvent` bus and via Tauri `app_handle.emit("contributions-changed", { kind })` for the frontend.

### B.4 — Spawn validation
- [ ] `service/orchestrator::spawn_agent[_sync]`: before building the launch command, look up `params.agent` in `registry`. If unknown → return `Err("Unknown agent provider: {id}. Available: {list}")`. Today any string is silently accepted and `build_agent_command` falls back to a no-op.
- [ ] Add a unit test asserting that spawn with `agent: Some("nonexistent".to_string())` returns Err.

### B.5 — Per-provider config schema
- [ ] In `src-tauri/src/config/schema.rs` add `pub agents: HashMap<String, toml::Value>` (or typed `AgentSettings { defaults: HashMap<String, ProviderSettings> }`) for `[agents.<id>]` sections.
- [ ] Schema example in default config TOML:
  ```toml
  [agents.claude]
  default_model = "sonnet"
  api_key_env = "ANTHROPIC_API_KEY"

  [agents.codex]
  default_model = "gpt-5"
  ```
- [ ] At config load, validate each `[agents.<id>]` section against the corresponding `provider.settings_schema()` (warn on unknown fields, error on type mismatch).
- [ ] `default_agent_provider()` helper stays as the global default; new helper `default_model_for(provider_id)` reads from `[agents.<id>].default_model` falling back to `provider.default_model()`.

### B.6 — Frontend: kill the hardcode

- [ ] Build the primitive first: `src/hooks/useContributions.ts`
  ```ts
  function useContributions<T>(kind: string, fetcher: () => Promise<T[]>): T[]
  ```
  Generic over contribution kind. Internally fetches via `fetcher`, caches in React context, refetches when a Tauri `contributions-changed` event arrives with matching `{ kind }`. **Forward-compat primitive for skills/commands/panels later — they'll be one-line wrappers.**
- [ ] Build `useAgentProviders()` as a one-line wrapper:
  ```ts
  export const useAgentProviders = () =>
      useContributions<ProviderDescriptor>('agent_provider', fetchAgentProviders);
  ```
- [ ] Replace `src/lib/agentProviders.ts` hardcoded registrations with calls to `useAgentProviders()`. `getAllProviders()` is removed; component code uses the hook directly.
- [ ] **Build the generic `<SchemaRenderer />` component** in `src/components/Settings/SchemaRenderer.tsx`:
  - One React component that takes `{ schema: SettingField[], configPath: string }` and renders any tree of fields
  - One sub-component per `SettingField` variant (`<SectionField/>`, `<ToggleField/>`, `<TextField/>`, `<SelectField/>`, `<InfoField/>`, `<PathLinkField/>`, `<BadgeField/>`, `<ActionField/>`, `<WhenField/>`, etc.)
  - `<ActionField/>` handles the dynamic state machine: on mount call `state_query`, render label/style from response, on click call `command`, refetch `state_query` after response
  - `<BadgeField/>` polls `query_command` on mount + on `config-changed` events
  - `<WhenField/>` reads the dependent config field from the same `configPath` namespace and conditionally renders children
- [ ] **Delete `ClaudeAgentSettings` and `CodexAgentSettings` React components entirely.** No legacy mode, no escape hatch, no `if (activeProvider === "claude")`. The provider sub-tab content becomes:
  ```tsx
  <SchemaRenderer schema={activeProvider.settings_schema} configPath={`agents.${activeProvider.id}`} />
  ```
- [ ] Move the **Skills** card (currently inside `ClaudeAgentSettings:868-884`) out of any single provider's panel into a global "Skills" section at the top of the Agents tab — it's a CodeTerm-wide feature that syncs to all providers, not Claude-specific.
- [ ] Audit and migrate the remaining hardcoded sites:
  - `src/components/Settings/SettingsPanel.tsx:735+` AgentsTab — uses hook ✓
  - `src/components/Settings/SettingsPanel.tsx:813-814` `if (activeProvider === "claude") { ... } else if (codex)` — replace with data-driven render of `provider.settings_schema` (one component per `SettingField` variant).
  - `src/components/Sidebar/Sidebar.tsx:750-757` "New X Session" menu — uses hook ✓
  - `src/store/multiplexer.tsx:605, 904` `spawnClaudeTab()` helper — rename to `spawnAgentTab(providerId)` or delete (callers pass id explicitly).
  - `src/components/CommandPalette/CommandPalette.tsx:33, 521-525` "spawn-claude" command — generate dynamically from `useAgentProviders()` (one command per provider).
  - `src/hooks/terminal/useAgentDetection.ts:51, 69, 72` `detect_claude_session_id` / `mux_set_claude_session_id` — keep as-is for now (Tauri command name; the actual generic rename happens in Chunk D).

### B.7 — Acceptance criteria for Chunk B

All items below were met on commit `3d0d772` (see the Chunk B DONE section above for the full commit chain and commit `6c7b9a2` for the frontend work specifically).

- ✅ Adding a hypothetical third native plugin (`plugin/agent_aider/`) requires ZERO frontend changes — it appears in Settings dropdown, Sidebar menu, CommandPalette, AND its own settings panel (rendered from its `settings_schema`) automatically after a restart.
- ✅ `src/components/Settings/SettingsPanel.tsx` contains no `ClaudeAgentSettings`, no `CodexAgentSettings`, no `if (activeProvider === "claude")` / `=== "codex"` branches. Provider settings rendered exclusively by `<SchemaRenderer />`.
- ✅ `src/lib/agentProviders.ts` is no longer used for Settings rendering — `useAgentProviders()` hook feeds everything. (The file still holds provider-specific parser functions referenced by ChatView; those will move to Chunk D or later when the plugin trait grows parser contributions.)
- ✅ Sending `POST /api/agents/spawn` with an unknown agent id returns an error before any side effects.
- ✅ `GET /api/agents/providers` returns identical JSON across REST, Socket, and Tauri (parity test via `dispatch::Command::ListAgentProviders`).
- ✅ All existing tests pass (399 passed). New tests cover trait metadata, round-trip golden, `deny_unknown_fields`, spawn validation.

## Next Up (priority order)

**Chunk C — Phase B3 dispatch batches.** Continue unified-dispatch migration, one batch per commit, parity tests before/after. Pure backend refactor; invisible to end users but kills ~2700 LOC of REST/Socket/Tauri duplication.

- [ ] **Workspace ops** — `WorkspaceCreate`, `WorkspaceRename`, `WorkspaceClose`, `WorkspaceList`
- [ ] **PTY ops** — `PtyResize`, `PtyKill`, `PtyExec`
- [ ] **Agent ops** — `AgentSpawn`, `AgentReport`, `AgentList` (the frontend-facing GA lifecycle commands). `ListAgentProviders` already landed in Chunk B.
- [ ] **Mesh ops** — `MeshPeers`, `MeshSendText`, `MeshAgentSpawn`
- [ ] **EventBus migration** (deferred from Phase C) — output-pipeline notifications + agent-status events emit typed `SystemEvent` variants instead of raw Tauri emits; piggyback on the contributions-changed bus bridge from `543bdc6`.

**Chunk D — Move Claude-specific HTTP/installer code into the plugin.** Unblocks the `api/routes.rs:1036` TODO and closes the last leaks where `src-tauri/src/agent/`, `service/`, `api/`, `commands.rs` still reference Claude directly. Requires one new plugin trait method: `register_routes(router: &mut Router)` so plugins can contribute HTTP endpoints on top of the core router.

- [ ] `Plugin::register_routes(router) -> Result<(), String>` trait method; `PluginManager` calls it at attach time and merges the returned sub-router under a namespaced path (e.g. `/plugins/<id>/...`).
- [ ] `commands.rs:675-700` (`~/.claude/rules/codeterm.md` install) → `plugin/agent_claude/`.
- [ ] `commands.rs:857-900` (Claude `~/.claude/settings.json` hook installer + `check_claude_hooks` + `uninstall_claude_hooks`) → `plugin/agent_claude/`.
- [ ] `api/routes.rs` Claude hook handler endpoint → route registered by `plugin/agent_claude/`.
- [ ] `multiplexer/commands.rs:197` `detect_claude_session_id` Tauri wrapper → rename to generic `detect_agent_session_id(provider_id, ...)`; keep the old name as a thin alias for one release for frontend compat.
- [ ] `src/lib/agentProviders.ts` — move the provider-specific parser function references (`parseJsonlSession`, noise regexes, image-send strategy) into a per-plugin module structure in the frontend. `useAgentProviders()` hook + a complementary `useProviderFunctions(id)` that pulls from a TS-side registry keyed by provider id.

After Chunk D: zero Claude/Codex literals in `src-tauri/src/agent/`, `service/`, `api/`, `commands.rs` outside of test fixtures. The core stops mentioning specific providers entirely; all provider-specific behavior lives under `plugin/agent_<name>/`.

## Guardrails (apply to all chunks until they're explicitly unlocked)

**Sync watcher hands-off.** Do NOT touch `src-tauri/src/service/sync_watcher.rs` from any of the plugin-prep chunks (B / C / D). Today it mirrors `~/.codeterm/skills/` into `~/.claude/skills/` with `clean_stale` — would silently delete user-installed Claude skills if exercised. Reverting and replacing with the bridge-rule generator is **Phase 3.0** inside the memory plugin work — see `docs/plans/memory-architecture.md`. Plugin-prep chunks must not introduce new mirror destinations or new dependencies on the existing mirror behaviour.

**Prompt regression testing.** When touching `src-tauri/prompts/*.md`, `src-tauri/src/prompts/`, or `.claude/agents/*.md`, run the `prompt-evaluator` skill (see `.claude/skills/prompt-evaluator/SKILL.md`) with `--prompt-a HEAD~1:<path> --prompt-b <path>` before merging. A/B report must show no regressions (Δ ≥ 0 on every eval). The harness is a maintainer tool, not bundled into CodeTerm.


## Current State → Target

```
CURRENT                                  TARGET (Hexagonal + Command Bus)
─────────────────────────────            ──────────────────────────────────
REST routes.rs (2225 LOC) ─┐             REST adapter (thin) ─┐
Socket server.rs (974 LOC) ┼─ duplicate  Socket adapter (thin) ┼─ Command enum
Tauri commands.rs (1547 LOC)┘            Tauri adapter (thin) ─┘     │
         │                                        │                   │
    ServiceContext (god object)              dispatch(Command, ctx)
         │                                        │
    All modules see all state              ┌──────┴──────┐
                                     ServiceContext    EventBus
                                           │               │
                                    ┌──────┼──────┐    Subscribers
                                    Core   │   Plugins
                                           │
                              ┌────────────┼────────────┐
                          Multiplexer    PTY         Mesh/Config
```

## Architecture: Hexagonal + Command Bus

**Why this pattern:**
- VS Code, Zed, Home Assistant, Neovim all converge on: typed command dispatch + event bus + adapter-per-transport
- Solves 3-API duplication (~2700 LOC eliminated)
- Plugins naturally register commands + subscribe to events
- Each operation testable in isolation (mock ServiceContext)
- Incremental migration — one command at a time

**Layers:**

| Layer | Contents | Depends on |
|-------|----------|------------|
| **Core** | Multiplexer, PTY, Mesh, Config, EventBus, PluginManager | Nothing external |
| **Commands** | `Command` enum, `dispatch()`, validation | Core |
| **Services** | Orchestrator, GA, Output pipeline | Core + Commands |
| **Adapters** | REST, Socket, Tauri IPC, CLI | Commands only |
| **Plugins** | Agent providers, Bridges, Memory | Core + Commands + EventBus |

**Dependency rule:** Adapters → Commands → Core. Never the reverse.

## Phase Plan

### Phase A: Test Foundation (this branch)
Cover critical untested code BEFORE restructuring. Tests are the safety net.

**A1. Clean up existing tests**
- [ ] Delete 6 useless tests (test_capitalize, test_tracker_set_dead, test_get_reports_empty, test_identity_has_keypair, duplicate registry detect tests)
- [ ] Fix 1 failing test (test_pty_kill in tests/pty_test.rs)

**A2. Cover zero-coverage critical modules**
- [ ] `service/output.rs` (429 LOC, 0%) — core output pipeline, ~20 tests
- [ ] `pty/screen_state.rs` — new get_screen_formatted, ~5 tests
- [ ] `service/pane.rs` (208 LOC, 0%) — pane CRUD, ~10 tests
- [ ] `service/workspace.rs` (128 LOC, 0%) — workspace ops, ~8 tests

**A3. Cover secondary modules**
- [ ] `service/notification.rs` (54 LOC, 0%) — ~5 tests
- [ ] `service/hierarchy.rs` (355 LOC, 14%) — role validation, ~10 tests
- [ ] `pty/write_queue.rs` — queue logic, ~5 tests

**Target:** 33% → 50%+ line coverage

### Phase B: Command Dispatch (foundation for plugins)
Extract command logic into unified dispatch. One operation at a time.

**B1. Define Command infrastructure**
- [x] Create `src-tauri/src/dispatch/mod.rs`
- [x] Define `Command` enum with first 5 operations (health, pane_list, pane_split, send_text, get_output)
- [x] Implement `dispatch(cmd: Command, ctx: &ServiceContext) -> Result<Value>`
- [x] Tests for each command in isolation

**B2. Migrate adapters incrementally**
- [x] REST: replace 5 route handlers with `dispatch()` calls
- [x] Socket: replace 5 method handlers with `dispatch()` calls
- [x] Tauri: replace 5 commands with `dispatch()` calls (`get_pane_output`, `get_pane_screen`, `get_pane_screen_formatted`)
- [x] Verify: all 3 adapters produce identical results for same input

**B3. Continue migration (remaining ~35 operations)**
- [x] Pane ops batch 1: `PaneSplit`, `PaneClose`, `PaneFocus`, `PaneRename` wired through REST + Socket (`36b5065`)
- [ ] Workspace ops
- [ ] PTY ops (resize, kill, exec)
- [ ] Agent ops
- [ ] Mesh ops

**Target:** ~4700 LOC → ~2000 LOC. One Command impl per operation.

### Phase C: Event Bus
Inter-module communication without direct coupling.

- [x] Add `EventBus` to ServiceContext (tokio broadcast channel — `EventBroadcaster::system_bus` in `src-tauri/src/api/events.rs`)
- [x] Define `SystemEvent` enum (`MuxStateChanged`, `PaneTitlePinned`, `PaneFocused`, `PaneClosed`, `PaneSplit`, `AgentStarted`, `AgentIdle`, `ConfigChanged`, `Generic`)
- [ ] Migrate `output.rs` notifications → EventBus publish (still call `emit_state_changed` only)
- [ ] Migrate agent detection status updates → typed `AgentStarted` / `AgentIdle` publishes (currently only defined, not emitted)
- [x] Frontend: EventBus → Tauri events (bridge via `app_handle.emit` in `emit_state_changed` / `emit_pane_title_pinned`)

### Phase D: Plugin Trait + First Plugin
From plugin-architecture.md Phase 1-2.

- [x] Define `Plugin` trait (`id`, `activate`, `deactivate`, `on_event`) in `src-tauri/src/plugin/mod.rs`
- [x] `PluginManager` — discover (`<dir>/*/plugin.toml`), register, `attach_native`, activate/deactivate, dispatch events filtered by `capabilities.hooks`
- [x] Expand `AgentProvider` trait with plugin-facing hooks (`install_skills`, `uninstall_skills`, `supports_lazy_skills`, `auto_approve_flags`, `compact_command`, `model_switch_command`, `exit_command`)
- [ ] Extract `agent-claude` into first native plugin (still in `src-tauri/src/agent/claude.rs`)
- [ ] Process plugin host (JSON-RPC stdio)
- [ ] WASM plugin host

## Test Strategy

### Per-phase testing

| Phase | Test approach |
|-------|--------------|
| A (foundation) | Unit tests on existing modules, no behavior changes |
| B (commands) | Each Command gets unit test. Adapter tests verify identical output before/after migration |
| C (event bus) | Integration tests: event published → subscriber receives |
| D (plugins) | Plugin trait mock tests. agent-claude plugin passes same tests as current inline code |

### Test organization

```
src-tauri/
  src/
    module/
      mod.rs          ← code
      (tests inline)  ← unit tests stay inline (standard Rust)
  tests/
    pty_test.rs       ← existing integration
    dispatch_test.rs  ← command dispatch integration tests (Phase B)
    plugin_test.rs    ← plugin lifecycle tests (Phase D)
```

### Coverage targets

| Milestone | Line coverage | When |
|-----------|--------------|------|
| Current | 33% | now |
| After Phase A | 50%+ | next |
| After Phase B | 60%+ | with command tests |
| After Phase D | 65%+ | with plugin tests |

## Key Principles

1. **Incremental.** No big bang rewrites. Each commit is a working build.
2. **Tests first.** Cover existing code BEFORE restructuring it.
3. **One command at a time.** Migrate operations individually, verify parity.
4. **SOLID at module level.** Each module has single responsibility, explicit dependencies.
5. **DRY via dispatch.** Three API surfaces share one implementation, not three copies.
6. **Plugin-ready.** Every architectural decision asks: "how does this work with plugins?"

## References

- `docs/plans/plugin-architecture.md` — full plugin spec
- `VISION.md` — project direction
- `docs/ARCHITECTURE.md` — current architecture + known issues
- VS Code Extension Host, Zed gpui::Action, Home Assistant integrations
