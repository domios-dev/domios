# Execution Board — open work (eng-manager revision 2026-06-19)

> Owner/manager: Dante. Workers: **Codex** (one task at a time, NO worktree —
> codex works in the main checkout on a `task/<name>` branch; the manager stays
> hands-off the repo during a worker's run, then gates + merges, then dispatches
> the next). Mine-only (mem) tasks are not delegated.

Mainline: `plugins-rethink` (@ 0c8f884b at audit time). Gate: pre-push hook
(clippy --all-targets + CLIPPY_ALLOW + fmt), authoritative.

## Wave 1 — CI red / hygiene (small, isolated, unblock CI) — ✅ DONE

- **W1.CI-green** (codex) — ✅ MERGED (55f1ba19): check-prompts-doc.sh → python3 +
  documented plugin_agent_task_preamble.md in PROMPTS.md.
- **W1.T10** (codex) — ✅ MERGED (0c69fb6a): split plugins.rs/graph_repair.rs/
  graph_repair tests/manifest.rs into 15+ files (all <1000) + bash-3.2-compatible
  check-file-sizes.sh + allowlist cleanup. 1731 lib + 8 pty tests green.

## Wave 2 — perf follow-ups (Kain) + guard wiring

- **W2.guards** (codex) — add `scripts/check-blocking-in-async.sh` (grep
  Command::new / blocking calls in async daemon commands, mirror check-mux-write.sh)
  AND wire BOTH check-blocking-in-async.sh + check-mux-write.sh into `.githooks/pre-push`
  (neither is wired today). 
- **W2.snapshot-singleflight** (codex) — single-flight the plugin snapshot cache
  (cold-start 4x stampede; harmless but ugly).
- **W2.host-net** (codex) — `host_net.rs` reuse a keep-alive client + drop the 30s
  fetch timeout to ~5s.
- **W2.build-mismatch-guard** (codex) — fix the "Command <x> not found" footgun.
  Root cause (hit live 2026-06-19: "Failed to spawn claude — Command
  mux_spawn_pane not found"): the dev web-watcher hot-rebuilds the FRONTEND dist on
  source change, but the Rust BACKEND (Tauri `generate_handler!` commands) only
  updates on a full daemon restart. So adding/renaming a command makes the
  frontend `invoke()` it before the backend registers it → cryptic "Command not
  found" until restart (amplified by a branch switch under a running daemon). The
  command WAS correctly registered (lib.rs:227) — pure build skew, not a code gap.
  Fix: a frontend↔backend build-hash handshake (the health `build` hash is
  trustworthy now after Kain's build.rs fix) — on connect / on an "unknown
  command" invoke error, compare the dist build hash to `/api/health.build`; if
  they differ, show a clear "backend is stale — restart dev (./build.sh dev)"
  banner instead of the cryptic toast. Files: src/lib/bridge.ts (invoke error
  path) + a small build-stamp emitted into the dist at build time.

## Wave 3 — bug fixes (Kain)

- **W3.T8** (codex) — file drag-drop: (1) materialize dropped macOS screenshot
  temp paths (`/var/folders/.../NSIRD_screencaptureui_*`) to a durable path before
  paste (attachments.rs save_with_*); (2) restore the window-wide "drop here"
  overlay (regressed to terminal-uiMode-only, TerminalPane.tsx:273; useTauriFileDrop.ts).
- **W3.T9** (codex) — codex chat empty ghost rows (chevron-only, 0-message sparse
  junk between exec_command rows) → render/parser fix; reproduce on a codex session.

## Wave 4 — chatBackend parity remainder

- **W4.P5** (codex) — syntax-highlight code inside diffs (DiffView reuse Prism,
  lang from file ext). Spec: 2026-06-19-chatbackend-pane-parity.md P5.
- **W4.P6** (codex) — model-switch JIT-load + VRAM confirm (plugin-side
  `describeModelSwitch` + generic UI confirm). Spec: same doc P6.
- **W4.S9-verify** (manager) — confirm `GET /api/panes/{id}/chat` no longer 400s
  for chatBackend + system_prompt card renders (mostly done by restore-fix). Live.
- **W4.D1-4** (codex) — preset-manager UX (frontend-design). **SUPERSEDED by Wave R
  (R5)** — do not run separately.

## Wave R — chatBackend / LM Studio robustness + UX (NEW, Andrey 2026-06-19)

Spec: `docs/plans/2026-06-19-chatbackend-robustness-ux.md`. Small local models
can't hold a strict tool-call format → tolerant Rust parser + nicer render +
simpler example-led prompt + flexible preset↔model binding + UX. Sequencing:
R1 → R2/R3 → R4 → R5 → R6.

- **WR.R1** (codex) — ✅ MERGED (7ae2f4c1) generic `host.toolcall.parse` (de-fence,
  native-syntax, JSON-repair, alias, schema-validate, golden+negative corpus,
  parity) + SDK type follow-up (8c64ef07). Plugin-wiring (lmstudio calls it) is
  **R1b**, queued below.
- **WR.R1b** (codex, plugins repo) — wire the lmstudio plugin to call
  host.toolcall.parse (pass its tool schema) instead of its fragile JS parser; emit
  structured tool calls. Repo: /Users/kandrey/Developer/codeterm-plugins (isolated
  checkout — no collision with codeterm-alpha). Pairs with R3.
- **WR.R2** (codex) — structured per-tool styled rendering in chat-ui.
- **WR.R3** (codex/plugin) — simpler example-led lmstudio prompt + per-model variant.
- **WR.R4** (codex/plugin) — flexible preset↔model binding (plugin-side).
- **WR.R5** (codex, frontend-design skill) — model bubble → terminal-bubble spot +
  preset manager (replaces W4.D1-4) + system-prompt card polish.
- **WR.R6** (codex/plugin) — author/refine a pane's system prompt via another pane.
- **WR.channels-ux** (codex, frontend-design skill; INDEPENDENT — plugins panel, not
  chat pane) — collapse the flat channel list into a master-detail "Channels" tab
  in the plugins panel (row: Search / All / Installed / Marketplace / Updates →
  add **Channels** next to Updates; default keep Updates). Click Channels → compact
  collapsed channel list (name + trust/official badge + plugin count). Select a
  channel → attributes in the RIGHT preview pane (url, version, trust, #plugins,
  last refresh). "Add channel" button → expands INLINE into an add form (name+url
  → validate → add), not a modal. Reuse existing channel data/API (verify channel-add
  exists in CLI/REST; wire the form). Seam: the T1 plugins-panel component. Gate:
  tsc + test:unit + design self-review.

## Wave 5 — big subsystems (design done, schedule after 1-4)

- **W5.T6.1-6 + T6.F** (codex, multi-task) — emergence migration: route the 6
  PTY-assuming sites through the AgentTransport seam + behavior-config UI per
  descriptor. Spec: 2026-06-19-t6-unified-provider.md.
- **W5.AS1-AS7** (codex, multi-task) — agent state classification + auto-advance
  (codex update auto-skip, claude rate-limit auto-resume, question/auth surfacing,
  missing-CLI install no-sudo). Spec: 2026-06-19-agent-state-autoadvance.md.

## Manager-only (mem / live, not delegated)

- **SR** — orchestration-skill rewrite + 2 dev-facts. Spec: 2026-06-19-orchestration-deburring.md.
- **FU1** — ledger `PRAGMA user_version` migration ladder (codex-able; small).
- **W4.S9-verify** — live check above.

## Not mine (untouched branches, Andrey's call)
release/1.1.13, feat/project-map (23 commits), feat/mesh*, feat/secrets-store,
fix/116-graph-edge-dedup, save/freeze-fix-*, perf-measure.

<!-- revision: 1 -->
