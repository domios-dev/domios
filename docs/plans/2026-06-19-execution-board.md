# Execution Board — open work (eng-manager revision 2026-06-19)

> Owner/manager: Dante. Workers: **Codex** (one task at a time, NO worktree —
> codex works in the main checkout on a `task/<name>` branch; the manager stays
> hands-off the repo during a worker's run, then gates + merges, then dispatches
> the next). Mine-only (mem) tasks are not delegated.

Mainline: `plugins-rethink` (@ 0c8f884b at audit time). Gate: pre-push hook
(clippy --all-targets + CLIPPY_ALLOW + fmt), authoritative.

## Wave 1 — CI red / hygiene (small, isolated, unblock CI)

- **W1.CI-green** (codex) — `scripts/check-prompts-doc.sh` calls `python` (missing;
  only python3) → use python3; AND document `src-tauri/prompts/plugin_agent_task_preamble.md`
  in `docs/PROMPTS.md` (the gap it flags). Bump PROMPTS.md revision. Files:
  scripts/check-prompts-doc.sh, docs/PROMPTS.md. STATUS: dispatched.
- **W1.T10** (codex) — split 4 oversized files by responsibility + fix
  `check-file-sizes.sh` bash-3.2 compat (`declare -A`) + update allowlist baselines:
  `mem/graph_repair/tests.rs` 1320, `mem/graph_repair.rs` 1077,
  `commands/plugins.rs` 1289, `plugin/js_runtime/manifest.rs` 1007 → each <1000.
  (Coupled: fixing the script reveals these violations.)

## Wave 2 — perf follow-ups (Kain) + guard wiring

- **W2.guards** (codex) — add `scripts/check-blocking-in-async.sh` (grep
  Command::new / blocking calls in async daemon commands, mirror check-mux-write.sh)
  AND wire BOTH check-blocking-in-async.sh + check-mux-write.sh into `.githooks/pre-push`
  (neither is wired today). 
- **W2.snapshot-singleflight** (codex) — single-flight the plugin snapshot cache
  (cold-start 4x stampede; harmless but ugly).
- **W2.host-net** (codex) — `host_net.rs` reuse a keep-alive client + drop the 30s
  fetch timeout to ~5s.

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
- **W4.D1-4** (codex) — preset-manager UX (frontend-design). Spec: same doc.

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
