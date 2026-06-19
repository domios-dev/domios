# Perf/UX Fix Wave — orchestration plan

> Codex/gpt-5.5 workers, each in its own git worktree off `plugins-rethink`.
> Workers report/push; the orchestrator gates (tsc + pnpm test:pw:lint + pre-push
> clippy) and MERGES every track into `plugins-rethink`. Root fixes only — NO
> band-aids. Measure/spec first. Final verify on Windows/ROG.
> node_modules: each worktree runs its OWN `pnpm install --frozen-lockfile` —
> NEVER symlink node_modules (it corrupted the shared pnpm store once).

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

## Discipline
- Each worker: own worktree, own `pnpm install` (no symlink), spec/measure → root
  fix → tsc + pnpm test:pw:lint (+ cargo/clippy for Rust) green → report.
- Orchestrator re-runs the gate itself, reviews diff, merges into plugins-rethink.
- Dev (7686) is a singleton — coordinate runtime between workers.
- Final verify on ROG (Windows, worst case).

<!-- revision: 1 -->
