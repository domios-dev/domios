# chatBackend Shell Panes — First-Class Parity + Durable History

> Orchestrated build. Workers: Opus (arch-heavy). One track per branch off
> `plugins-rethink`, STRICT TDD, commit before reporting, gate with the
> **pre-push hook clippy** (`cargo clippy --manifest-path src-tauri/Cargo.toml
> --workspace --all-targets -- -D warnings` + the CLIPPY_ALLOW `-A` flags from
> `.githooks/pre-push`) — `--lib` misses test-target lints. No band-aids.

**Goal:** Make chatBackend SHELL panes (lmstudio etc.) first-class like agent
panes, and give them DURABLE chat history (they have no provider-side store).

**Root insight:** the frontend resolves pane identity (icon, name, model, model
switching) from `pane.agent_type` (agent registry). Shell panes have NO
`agent_type` — their provider is `pane.backing.plugin_id`, and the plugin's
metadata (icon/models) already exists in the plugin snapshot but is never joined.
And their transcript lives ONLY in the plugin VM's in-memory sessions Map → lost
on refresh / VM reload / daemon restart. Fix both at the source.

## Status
- **T1 DONE** (`58cd63ca`): `+ new session` opens shells via `open_chat_backend_pane`
  (ProviderDescriptor.kind='shell' routing), not the PTY launch path.

---

## P1 — backend: expose plugin identity + models + live session model for shells — branch `cb-p1-meta`

**Role:** Rust backend.

So the frontend can resolve a shell pane's icon/name/models by `backing.plugin_id`
and show/switch the live model.

- Ensure the plugin snapshot (`plugin_manager_snapshot` / `commands/plugins.rs`)
  exposes per-plugin `icon_svg`, `icon_color`, `icon_hint`, `display_name`,
  `available_models` (from `listModels()` / catalog), `default_model`. (Some are
  already present — add the missing ones.)
- New command + API: `chatbackend_session_info(plugin_id, session_id)` →
  `{ model }` (current model of the live session) so the tab/bubble can show it.
- New command + API: `chatbackend_switch_model(plugin_id, session_id, model)` —
  switch the model for an open shell session. Add an OPTIONAL `setModel(sessionId, model)`
  export to the chatBackend capability (`chatbackend/mod.rs` trait + JS adapter +
  SDK type); the lmstudio plugin implements it (updates session model used on the
  next `/api/v1/chat`). Core asks the capability, never a hardcoded id.
- Do NOT touch `pane_chat/mod.rs` (P4 owns it).
- TDD: snapshot includes the new fields; session_info returns the model;
  switch_model contract test (call_json) routes to the plugin export.
- Gate: cargo fmt/build/test (plugins, chatbackend) + hook clippy + tsc (SDK).

## P2 — frontend: shared pane-provider resolver + tab affordances — branch `cb-p2-tab` (dep P1)

**Role:** React/TS.

A single helper `resolvePaneProvider(pane)` → `{ id, displayName, iconSvg,
iconColor, iconHint, kind }` from EITHER `agent_type` (ProviderDescriptor) OR
`backing.plugin_id` (plugin snapshot cache). Route all of these through it so
shell panes light up like agent panes:
- **Tab icon** (`Sidebar/TabItem.tsx:190`): render the plugin's `icon_svg`/`color`
  for a chatBackend pane (not the plain TerminalIcon); include the working
  animation branch.
- **Tab tooltip** (`TabItem.tsx:191`): plugin display name, not generic "Agent".
- **Model badge on tab** (`TabItem.tsx:269`): show the shell session's model
  (from P1's `chatbackend_session_info`) when `agent_session_model` is null.
- **Chat-UI toggle + chat-view default** (`chatUiMode.ts` `isChatUiSupported` /
  `shouldDefaultNewAgentPaneToChat`): account for `backing.kind === 'chat_backend'`
  (shells are always chat).
- TDD: resolver unit tests (agent id, shell id, unknown); the gates account for backing.
- Gate: tsc + test:unit.

## P3 — model-picker bubble for shells — branch `cb-p3-bubble` (dep P1)

**Role:** React/TS + small backend glue.

The user's ask: a bubble at the TOP of the chat (like the terminal had) showing
the current model + a dropdown (up-arrow) to switch which model the chat uses.
- A bubble/header for chatBackend chat panes (mirror the agent model-picker UX in
  `ChatComposer.tsx` / `PluginBubbles.tsx`) showing the current model (P1
  `session_info`) + a dropdown of `available_models` (P1) → on pick call
  `chatbackend_switch_model` (P1). Reflect the change in the tab badge (P2).
- Keep it generalized: any chatBackend shell with `available_models` gets it; no
  hardcoded lmstudio.
- TDD: the bubble renders current model + lists models + invokes switch.
- Gate: tsc + test:unit.

## P4 — durable history (JSONL) + restore — branch `cb-p4-history`

**Role:** Rust backend. **Most critical** (history loss = worst bug).

Persist shell transcripts host-side (the plugin has no durable store). Use
**JSONL per session** (consistent with the agent-session model; reuses the
snapshot/parse path; no schema; human-readable). Decision locked: JSONL, not SQL.

- **Write hook:** a `ChatBackendStructuredSink` wrapping the Tauri sink; in
  `spawn_chatbackend.rs` the poll loop appends each emitted `ChatMsg` (UPSERT by
  id — a streaming message grows in place, so rewrite its line / compact, do NOT
  duplicate) as JSONL to `~/.codeterm[-dev]/chatbackend-sessions/<plugin_id>/<session_id>.jsonl`
  (via `paths::data_dir()`). Best-effort, non-blocking, no fsync on hot path.
  Reuse the iter1 delta/dedup so we only write real changes.
- **Restore (refresh):** `chatbackend_transcript_snapshot` (`pane_chat/mod.rs`)
  reads the JSONL (durable truth) instead of relying on the live plugin Map, so a
  refresh / VM reload shows full history. Honor `since`.
- **Restore (daemon restart):** persist `backing: PaneBacking::ChatBackend` in the
  session snapshot (`multiplexer/state/snapshot.rs` — currently dropped, comment
  "R3 restore-gap"); on restore, re-attach backing; lazily re-`openSession` on
  first send (re-seed the plugin from the persisted system-prompt/preset so the
  conversation can continue). The system-prompt (preset) is the first JSONL line.
- Account for upsert-by-id so the JSONL doesn't bloat with streaming snapshots
  (compact the growing live message; or write only finalized messages + the live
  one on completion).
- TDD: write a message → JSONL has it; snapshot reads a hand-written JSONL; a
  streaming message yields ONE final line not N; restored pane replays history.
- Gate: cargo fmt/build/test (pane_chat, chatbackend, snapshot) + hook clippy.

---

## Dependencies / order
`T1 done`. **P1 + P4 in parallel** (backend, disjoint files: P1 = plugin snapshot
+ session_info/switch commands; P4 = sink/spawn_chatbackend/pane_chat snapshot +
snapshot.rs — P1 must NOT touch pane_chat). Then **P2 + P3** (frontend, dep P1).
Orchestrator gates + merges each (hook clippy); rebuild dev + live-verify after each.

## P5 — syntax-highlight code inside diffs — branch `cb-p5-diff-hl` (frontend, independent)

**Role:** React/TS.

`DiffView` (`packages/chat-ui/src/MarkdownBody.tsx:198`) renders Edit/MultiEdit
diffs with line-level +/- coloring (red removed / green added) but the code inside
is plain monospace — no token highlighting. `CodeBlock.tsx` already uses Prism
(`react-syntax-highlighter`, vscDarkPlus) for fenced blocks, so the highlighter is
already a dependency. FIX: in DiffView, run each line's content through the same
Prism highlighter (strip the +/- marker, highlight the content, keep the +/- line
background), with the LANGUAGE inferred from the edited file's extension (the Edit
tool carries the path; e.g. `TabItem.tsx` → `tsx`). Performance: highlight the
hunk content once (not per-render); for very large diffs cap/virtualize as CodeBlock
already does. Keep the +/- gutter + line backgrounds; only the token colors are added.
- TDD: DiffView renders a tsx diff with highlighted tokens on +/- backgrounds; an
  unknown extension falls back to plain (no crash); large-diff cap respected.
- Gate: tsc + test:unit. Independent of CB-P2/P3 (different file) — dispatch after
  the CB-P2/P3 frontend batch merges to avoid chat-ui churn.

## P6 — model-switch with load + VRAM confirm (PLUGIN-SIDE, generic UI seam) — branch `cb-p6-switch-load`

**Role:** QuickJS plugin + small React/TS. **Hard rule: ALL switch/load/VRAM logic
lives in the lmstudio PLUGIN; core stays generic (no hardcoded lmstudio/model logic
— verified clean: core gets models via the `list_models` capability, switch via
`setModel`).**

The user wants: switching the model in the lmstudio shell should (auto-)load it in
LM Studio, and if it would unload the current one / VRAM is tight, ASK "switch
model? the current one will be unloaded" before doing it.

Reality (probed live): LM Studio's `POST /api/v1/models/load` returns **404** on the
current build — there is no explicit REST load. BUT LM Studio **JIT-loads** a model
when a chat request references it (and unloads per its own limit). So:
- **lmstudio plugin `setModel`**: sets the session model; the next `/api/v1/chat`
  JIT-loads it. Catch a load failure (VRAM/insufficient) from that response and
  surface a clean `system` message ("couldn't load <model>: not enough VRAM —
  unload a model in LM Studio or pick a smaller one").
- **Plugin decides if a confirm is needed** (the user's requirement: the PLUGIN
  understands the cost): add a plugin method (e.g. `describeModelSwitch(sessionId,
  model) -> { needsConfirm: bool, message: string }`) that returns whether the
  switch will unload/load (e.g. target not currently loaded) + a human message.
  The model bubble (CB-P3) calls it on pick; if `needsConfirm`, show a GENERIC
  confirm dialog with the plugin's message; only on OK call `chatbackend_switch_model`.
  The confirm UI is generic (any chatBackend shell can provide a switch message);
  the DECISION + text come from the plugin → zero lmstudio specifics in core/UI.
- If the explicit `/api/v1/models/load`/`/unload` becomes available (version/path),
  the plugin can pre-load proactively — but keep JIT as the baseline.

TDD: plugin describeModelSwitch returns needsConfirm when target unloaded; setModel
surfaces a VRAM load-failure; bubble shows confirm + only switches on OK. Gate:
plugin build/test/tsc + tsc/test:unit + hook clippy. Independent-ish; after CB-P3.

## Verify (orchestrator, live on dev + LM Studio)
- Shell tab shows the lmstudio ICON + the running MODEL; tooltip = "LM Studio".
- Model bubble at top: switch model → next reply uses it + tab badge updates.
- Send messages, **REFRESH the chat → full history persists**; restart dev → the
  shell pane comes back with its history (+ can continue).
- `+ new session` → lmstudio opens (T1). codeterm-npc still excluded from spawn.

<!-- revision: 1 -->
