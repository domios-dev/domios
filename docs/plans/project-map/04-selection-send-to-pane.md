# Phase 4 — Selection + send-to-pane action bar

**Status**: plan
**Owner**: Mac-Air (will spawn worker after Phase 3 Step B lands)
**Depends on**: Phase 3 canvas (must be live — selection rides on top of it).

## Goal

Turn the canvas from a viewer into an **operator surface**. The user multi-selects nodes, types a prompt, picks (or auto-picks) a target pane, hits Send — the prompt + structured-block payload arrives in the chosen pane as a piece of work. From-the-other-direction: copy-as-block writes the same payload to the clipboard for paste-into-chat scenarios.

This is the moment the project-map stops being a documentation tool and starts being a CodeTerm primitive: "I think in modules and connections; do this thing across these pieces."

## What ships

1. **Selection model** in the canvas (Cmd/Ctrl-click adds, Shift-click range, marquee drag, Esc clears).
2. **Floating action bar** that slides up when selection is non-empty.
3. **Pane picker** with persisted "last dispatch pane" per workspace.
4. **Structured-block composer** that builds the message body from selected nodes.
5. **Clipboard fallback** ("Copy as block") for chat-paste flows.
6. **Send semantics**: dispatched message goes to chosen pane via existing `pane.send_text` / REST.

No new heavy backend; almost all work is frontend. The one backend piece is persistence of the per-workspace "last dispatch pane".

## Backend additions

### Per-workspace KV store for `last_dispatch_pane`

The cleanest path is a small new table inside the project-map SQLite (already per-workspace). Or — even simpler — a JSON sidecar `<workspace>/.codeterm/map/v1/state.json`:

```json
{
  "last_dispatch_pane": "abc1234",
  "last_message_template": "optimize this — too much coupling here"
}
```

CLI/REST not needed; frontend reads/writes directly via Tauri command (or via the existing daemon REST `/api/map/state` to keep frontend Tauri-free).

A new daemon endpoint:

```
GET  /api/map/state?workspace=WS_ID                          → { last_dispatch_pane, last_message_template }
PATCH /api/map/state?workspace=WS_ID  body { ... }           → echoes the merged state
```

Implemented in `src-tauri/src/api/map_routes.rs` (already exists for `/api/map/graph` + `/api/map/show`).

### Send-to-pane is the existing primitive

We already have `cmd_send_text` / `pane.send_text` (used today by `codeterm send-text`). Phase 4 frontend just calls it through the Tauri invoke or REST. Nothing new on the daemon for the actual dispatch.

### Eligible-target query

Selecting from "all panes" in the workspace would surface workers (which shouldn't take new tasks). Add a small filter helper in `src-tauri/src/service/pane.rs`:

```rust
pub fn dispatchable_panes_in_workspace(&self, workspace_id: &str) -> Vec<DispatchablePane>;
```

Returns: orchestrators + leads, with title + agent_type, in pane order. Frontend renders this as the pane-picker dropdown.

## Frontend work

```
src/components/ProjectMap/
  selection/
    SelectionContext.tsx               # provider, exposes `selected: Set<NodeId>` + selection ops
    useSelection.ts                    # hook with click / cmd-click / shift-range / marquee
    MarqueeOverlay.tsx                 # absolute-positioned drag rectangle
  ActionBar.tsx                        # floating action bar (slide-up animation)
  PanePicker.tsx                       # combobox of dispatchable panes
  composer.ts                          # buildMessage(selection, prompt) → string
  hooks/useDispatchState.ts           # GET/PATCH /api/map/state
  hooks/useDispatchablePanes.ts        # GET /api/panes filtered to orchestrator/lead
```

Each file ≤ 250 lines, ActionBar ≤ 350.

### Selection states

| Action | Effect |
|---|---|
| Click empty canvas | Clears selection AND focus |
| Click node | Single-select + sets focus (Phase 3 behavior preserved) |
| Cmd/Ctrl-click node | Toggles node in selection (focus unchanged) |
| Shift-click node | Range-select between current focus and clicked node, by graph BFS distance — minor work; v1 = simple union of focus + clicked + 1-hop intersection |
| Drag empty canvas | Marquee — every node whose center falls in the rect is added |
| Esc | Clears selection (focus stays) |

Selected nodes get a thick `--ct-accent` ring (radius +2px) and stay full opacity even when faded by focus mode.

### ActionBar

Slides in from the bottom 80ms ease-out when `selection.size > 0`. Layout:

```
┌────────────────────────────────────────────────────────────────┐
│ 3 nodes selected             [•••]  show       Cancel | Send  │
│                                                                │
│ Send to: [▼ Orchestrator (pane abc1234)]                       │
│ Message:                                                       │
│ ┌──────────────────────────────────────────────────────────┐   │
│ │ optimize this — too much coupling here                   │   │
│ └──────────────────────────────────────────────────────────┘   │
│                              [📋 Copy as block]   ⇧⌘↵ Send    │
└────────────────────────────────────────────────────────────────┘
```

`[•••]` show button → expands to list each selected node (`path:line — display_name`).

### Composer payload format

```text
<user prompt>

--- Selected nodes (3) ---
- src/components/Settings/Panel.tsx:42 — Panel (function)
- src-tauri/src/service/pane.rs:78 — open_project_map (function)
- src-tauri/src/service/project_map/manager.rs:14 — ProjectMapManager (class)

--- Focus context ---
Currently focused: src/App.tsx:127 — App
Edges between selection: 2 invoke, 1 contain
```

### Pane picker

- Default value: `state.last_dispatch_pane` if it still exists in `dispatchable_panes_in_workspace`, else first orchestrator, else first available.
- If only one eligible pane → no dropdown, just label.
- If picker default hits a stale ID → silently fall back; no error toast.

After Send: persist `state.last_dispatch_pane = picked.id` and `state.last_message_template = prompt` (the latter helps with iterative edits).

### Copy-as-block

Renders the same composed string, runs `navigator.clipboard.writeText`. Toast: "Copied 3 nodes". No daemon involvement.

## Acceptance criteria

**Functional**
- Open the map for this repo. Click a node, then Cmd-click two more. Action bar slides up showing "3 nodes selected".
- Type a prompt, click Send. The Orchestrator pane in this workspace receives the composed message.
- Closing the action bar (Esc) clears selection but preserves focus.
- After a successful Send, re-opening selection auto-fills the Send-to to the same pane.
- "Copy as block" places the same payload on the clipboard.
- Closing the previously chosen pane and re-opening selection silently falls back to the first orchestrator.

**Structural**
- Pane filter for dispatchable targets lives in `service/pane.rs` and re-exports cleanly to REST.
- All new TS files ≤ 350 lines.
- TS strict typecheck clean. Rust check clean.

**Tests**
- Composer pure-function unit test: given selection + focus, produces expected string.
- Selection hook unit test: click / cmd-click / esc exercise.
- Rust unit test for `dispatchable_panes_in_workspace` (orchestrator and lead included; worker excluded).

## Patterns borrowed

| Pattern | Source | Usage |
|---|---|---|
| Combobox / dropdown chrome | existing `src/components/Sidebar` DropdownMenu | PanePicker |
| Composer payload structure | inspired by SourceTrail "Custom Trail" + this repo's existing `cmd_send_text` body | `composer.ts` |
| Per-workspace state JSON sidecar | Phase 2's `<workspace>/.codeterm/map/v1/config.json` | `state.json` next to it |

## Risks / open questions

1. **Marquee selection complexity.** Implementing rect-pick on a Sigma WebGL canvas needs `viewport-to-graph` coordinate math. Sigma has `graphToViewport` / `viewportToGraph` helpers — use them. Defer marquee to a follow-up commit if it slips.
2. **Send-to-pane race.** If the picked pane closes between selection-time and send-click, the existing `pane.send_text` will return 404. Catch and toast; reopen picker.
3. **Auto-routing risk.** The "last dispatch pane" auto-pick is opinionated. Keep an explicit "[change]" affordance always visible.
4. **Pane ordering for picker.** Use the order panes appear in the workspace (left→right, top→bottom). Tie-break by recency.

## Out of scope (Phase 4)

- Drag-and-drop sending (drop selection onto a pane in the sidebar) → backlog.
- Saved selections / "scenes" → backlog.
- Range-selection by graph distance (not Euclidean) → simple-union v1; smarter version later.
- Sending to remote mesh peers → Phase 9+.

## Done definition

- All acceptance criteria met.
- Plan + code committed on `feat/project-map`.
- Manual e2e: pick three nodes, type a prompt, send — see it land in the chosen pane.
- `codeterm agent report "phase 4 selection-dispatch done: …"`.
