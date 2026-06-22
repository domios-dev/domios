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

## Wave M — Messenger gateway (storage-aware inter-pane routing, NEW 2026-06-19)

Spec: `docs/plans/2026-06-19-messenger-gateway.md`. Bug: a `send --pane` into a
chatBackend (non-PTY) pane arrives as a BARE bubble — sender identity/return
address lost (the prefix is stripped along with the `<context>` block). Fix:
promote `CtxPrefix` → a `Messenger` gateway that routes on WHERE the chat is
stored (PTY=inject text prefix; chatBackend=store structured inbound msg with
sender metadata). All senders (send/report/mesh/delayed) go through it.
- **WM.M1** (codex) — Messenger gateway backend (rename msg_prefix→messenger,
  MessageEnvelope, deliver(), recipient-storage branch, route send+report).
- **WM.M2** (codex) — chatBackend inbound "from <label·model>" rendering (frontend).
- **WM.M3** (codex) — route delayed/reminder + mesh through Messenger.
- **WM.M4** (codex/plugin) — chatBackend reply-to-return-address.
- **WM.M1b** — ✅ MERGED (389ad403): dedup inbound + canonical -=-codeterm:inbound-=- marker + reply identity via per-session host.exec env (this likely SUBSUMES M4 reply-to-return-address — verify). ORIG: gateway ROBUSTNESS (live bugs): de-dup inbound (one entry not two), canonical -=-codeterm:-=- marker (not [CodeTerm inbound]), reply carries chatBackend pane identity. TDD. Spec: messenger-gateway.md M1b.

## Wave TH — reopen closed tabs (history) — NEW 2026-06-19

Remember closed tabs so they can be raised again (browser-style). Spec drafted.
- **TH1** (codex, backend) — per-workspace "recently closed" ring (pane_id, title,
  PaneBacking, cwd, model, closed_at); persist in snapshot (reuse CB-P4 backing
  persistence); `reopen_closed_pane(workspace[,index])` command/REST. chatBackend
  reopen replays JSONL history (restore-fix lazy-reopen). Bounded (~20).
- **TH2** (codex, frontend) — "Recently closed" UI + Cmd+Shift+T.

## Wave TD — tab description (≤200-char "what it's doing") — NEW 2026-06-19

- **TD1** (codex, backend+CLI+FE) — pane `description: Option<String>`, server-side
  VALIDATION reject >200 chars (+ CLI client-side); `codeterm pane describe "…"` +
  `agent spawn --description "…"`; in pane list/status; persist in snapshot; show as
  tab tooltip/subtitle. TDD: 200 ok / 201 rejected / snapshot round-trip / spawn sets it.
- **TD2** (skill) — update the codeterm skill ("Auto-name your tab" section) to also
  instruct agents to set a ≤200-char description (at spawn / on start).

## Wave E — emergence wiring (cross-pane reply semantics, NEW 2026-06-19)

From live screenshots: a PTY agent (GA) told a chatBackend pane to "run codeterm
send in your shell" — but a chatBackend pane is NOT a shell (it acts via curated
tools). And inbound messages arrived with no sender (M1 not yet deployed at the
time). Fix both sides.
- **WE.E1** (plugin) — ✅ MERGED (plugins bcf9a12): lmstudio prompt teaches the
  model to reply to an inbound `[CodeTerm inbound … from_pane=…]` via the codeterm
  tool with the CORRECT arg shape (`{"args":{"args":"send \"…\" --pane <id>"}}`).
- **WE.E2** (codex/prompt) — PTY agents (GA + the codeterm skill / general_agent
  prompt) must understand pane TYPES: a chatBackend peer replies via its tool, NOT
  a shell. Stop instructing chatBackend peers to "run in your shell"; describe how
  to address chatBackend vs PTY panes. Surface in the shared agent prompt/skill.
- **WE.M4** (= Wave M M4) — the formal reply-to-return-address routing closes the loop.

## Wave U — UI polish

- **WU.model-badge** (codex) — long LM Studio model ids on sidebar tabs (e.g.
  `GOOGLE/GEMMA-4-26B-A4B-QAT`) overflow + cover the tab name. Strip vendor prefix
  (before `/`), drop quant/format suffixes (-QAT/-A4B/-MLX/-4bit/…) when over a cap,
  hard max-width + ellipsis so the NAME always shows; full id in tooltip. Pure
  formatter + TabItem. Gate: tsc + test:unit.

## Done this session (merged to plugins-rethink)
Wave1 (CI-green, T10) · R-chain (R1 parser, R1b+R3 plugin, R2 cards) · Messenger
(M1 gateway, M2 inbound render) · E1 reply-prompt. dev @5667c0c5 deploys all of it
+ refreshed lmstudio plugin. R2 cards confirmed live by Andrey.

## Manager-only (mem / live, not delegated)

- **SR** — orchestration-skill rewrite + 2 dev-facts. Spec: 2026-06-19-orchestration-deburring.md.
- **FU1** — ledger `PRAGMA user_version` migration ladder (codex-able; small).
- **W4.S9-verify** — live check above.

## Not mine (untouched branches, Andrey's call)
release/1.1.13, feat/project-map (23 commits), feat/mesh*, feat/secrets-store,
fix/116-graph-edge-dedup, save/freeze-fix-*, perf-measure.

<!-- revision: 1 -->

## Wave CB — core bugs from Kain (cursor-provider work) — NEW 2026-06-19

- **CB181** (issue #181) — agent session-bind cross-contaminated by foreign provider sids -> blank chat. Fix bind_reported_session (hook_routes.rs:~365): verify sid BELONGS to provider_id (provider.session_exists), reject cross-provider, tighten missing-nonce tolerance. DISPATCHED (codex, wk-cb181). Disjoint from in-flight.
- **CB182** (issue #182) — send to a lazy/not-materialized agent pane writes to the RAW shell. Fix send_text (pty_io.rs:~82): if backing=Agent and CLI not running, materialize+await readiness (reuse mount-time resume) BEFORE the PTY write; same guard for taskDelivery=pty_input, report-injection, mesh delivery, --delay fire. SEQUENCE AFTER M3 (both touch send/delayed/mesh paths).

- **WM.M1c** (opus) — gateway bugs round 2 (live): (A) inbound ChatMsg.content carried the raw -=-codeterm:inbound-=- marker -> store CLEAN body, marker only to model via staged; (B) two ChatBackendHistory writers (deliver_chatbackend + poll loop) clobber the inbound JSONL row -> single shared writer / merge-on-flush so inbound+reply survive refresh. TDD. Spec: messenger-gateway.md.

- **WR.R8** (multi: core Rust + plugin) — tool-call resilience for weak-model broken JSON: measure corpus -> structural reply-tool (prevention) -> core repair_json strengthen + re-validate -> core malformed-signal + plugin feedback/retry (kill silent drop) -> tests -> metrics. Spec: chatbackend-robustness-ux.md R8. NOT started.

## Wave CR — cascade + workspace restore (extends reopen-closed) — NEW 2026-06-19

Builds ON the sqlite closed-history (task/closed-history-sqlite, cd751800) — SEQUENCE AFTER it merges (same area). Idea: restore GROUPS, not just single panes.
- **CR1** (backend, Opus) — group the sqlite closed-history: a `closed_group` (group_id, kind = single|cascade|workspace, label, closed_at); closed_panes rows reference group_id + their parent_pane_id (cascade hierarchy) / workspace metadata.
  - CASCADE close (orchestrator + spawned children closed together) -> record as ONE group preserving parent/child links; reopen restores the whole subtree + re-links hierarchy (parent_pane_id) + each pane's backing.
  - WORKSPACE close (whole workspace + its tab list) -> record the workspace + its panes as a group; reopen re-creates the workspace with all tabs.
  - New command/REST/socket: reopen_closed_group(group_id) (+ keep single reopen). Bounded + sqlite (no snapshot bloat).
- **CR2** (frontend, frontend-design skill) — RecentlyClosed UI represents GROUPS: a cascade shows as one restorable entry ("Orchestrator + 3 workers"), a workspace as one ("Workspace 'General' — 5 tabs"); expandable to restore the whole group OR an individual item; visually distinct from single-pane closes. Use frontend-design sensibility (clear grouping, not templated).
- TDD: cascade close records a group with hierarchy; reopen restores the subtree + parent links; workspace close+reopen round-trips the tab list; single closes unchanged. Gate: cargo + hook clippy + guards (CR1); tsc + test:unit + design self-review (CR2).
- Sequence: cd751800 -> CR1 -> CR2.
