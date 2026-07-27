# Canvas — Design Spec

<!-- revision: 2 -->

**Status:** Draft for review
**Scope:** Full feature — backend data model + storage + CLI + REST/socket + prompts/docs + frontend canvas. Local-per-machine v1; mesh-sync deferred but data model kept mesh-ready.

## 1. Concept

Canvas is a Figma-like 2D board, scoped to a workspace, that visualizes live orchestration: the orchestrators working in a workspace, the **plans** they own, and for each plan the cascade **Spec → Tasks → chain of report-milestones**, where chains loop into a self-improving process. It is the spatial, persistent counterpart to the orchestration that today exists only as ephemeral chat + the report ledger.

It is not a new orchestration engine. The hierarchy (Orchestrator → Worker), reports, and Tab model already exist. Canvas adds a **persistent plan object**, **CLI handles to edit it**, and a **2D rendering** of plans and their report chains, plus two entry points into that view.

### What already exists (discovery)

- **Reports persist**: in-memory `ReportStore` + durable `~/.codeterm/orchestration.db` (one upsert row per orchestrator↔worker). `codeterm agent report` already carries `message, status, branch, sha, changed_files, test_cmd, test_result, skipped` and routes to the owning orchestrator (`src-tauri/src/service/hierarchy.rs`, `src-tauri/src/agent/reports.rs`).
- **Role/hierarchy model**: `AgentRole = Orchestrator | Worker`, `parent_pane_id`, `depth`, `can_spawn`, `report_target_pane_id`, `spawned_by_pane_id` on `Pane` (`src-tauri/src/multiplexer/types.rs`). `CODETERM_ROLE` env set at spawn.
- **A proto-canvas exists**: `src/components/Swarm/` (SwarmOverlay, Canvas-2D orchestrator→worker tree with animated "report particles" along edges) — currently commented out (`App.tsx` TODO). Its report-particle metaphor matches the milestone chain and can be reused for chain animation later.
- **Three canvases in code**: Mesh (`MeshCanvas.tsx` — absolutely-positioned divs + CSS-transform pan/zoom, custom), Memory (`MemorySnapshotCanvas.tsx` — Sigma WebGL graph), Swarm (Canvas2D). No shared pan/zoom hook.
- **No plans anywhere**: no plan struct, table, file, or per-project `.codeterm/`. Everything lives in global `~/.codeterm` (`-dev` in dev).
- **Mount points (precise)**: workspace empty-state → `App.tsx` `<main>` (~543–649); Plan button → `PaneTopBar.tsx` right of `PaneModeSwitch` (~52–54).
- **Prompt seam**: `src-tauri/prompts/codeterm_cli.md` (injected via `{codeterm_cli}`), `orchestrator_suffix.md` / `worker_suffix.md` (short imperatives); orchestration *principle* lives in mem skill `codeterm-orchestrator`.

## 2. Decisions (locked)

| # | Decision | Choice |
|---|----------|--------|
| 1 | Plan storage format | **JSON, one file per plan** at `<project>/.codeterm/plans/<plan_id>.json`. Daemon is the sole writer. |
| 2 | Canvas render tech | **Mesh approach**: absolutely-positioned DOM/markdown cards + CSS-transform pan/zoom, extracted into a shared `useCanvasViewport` hook (retrofit Mesh onto it). No new rendering dependency. |
| 3 | Build scope | **Full** spec + full plan. Internal phasing, no feature drop. |
| 4 | Mesh sync | **Local per-machine v1.** Data model kept mesh-ready (stable ids, owner identity, `updated_at`). |

## 3. Data model

Core entities nested under a Plan. Source of truth = the per-project JSON file; an in-memory index (no database — see §3.7) enables fast cross-project listing.

### 3.1 Plan
```
Plan {
  id: string            // stable uuid, mesh-safe
  title: string
  sections: Section[]   // structured spec — the left cascade; tasks attach to sections (see below)
  status: "active" | "archived"
  owner: OrchestratorRef // stable identity (see 3.4)
  project_path: string  // git toplevel / workspace cwd the plan belongs to
  created_at: u64
  updated_at: u64
  milestone_kinds: MilestoneKind[] // declared at plan-formation; the chain's stage vocabulary / columns
  loops: LoopSpec[]     // declared self-improving cycles, optional/empty (see 3.6)
  elements: CanvasElement[] // free-form extensible nodes (notes, mockups, …) — see 6.3; empty by default
  task_groups: TaskGroup[] // optional lightweight grouping of tasks (≈ phases); empty by default (see 3.8)
  links: string[]       // ids of related/parent plans — derived-from / discovered-during; an edge between plan frames (see 3.8)
  tasks: Task[]
  canvas: CanvasLayout  // persisted positions/hints (see 3.5)
}

MilestoneKind {
  key: string           // e.g. "testing"
  label: string         // e.g. "Testing results" (rendered column header)
  color: string | null  // optional accent
}

TaskGroup {
  key: string           // e.g. "phase-1"
  label: string         // e.g. "Phase 1 — backend"
  color: string | null  // optional accent
}
```
```
Section {
  id: string
  title: string
  body: string          // markdown
  subsections: Section[] // nested; a plan is always a tree of sections/subsections
  canvas: ElementCanvas  // each section is its own canvas node
}
```
The spec is **structured**, not a flat blob: a plan is a tree of sections/subsections, each independently editable. `plan create --spec-file SPEC.md` auto-splits the markdown by headings into sections. Tasks attach via `Task.section_ids` (many-to-many), so each task covers one or more specific sections.

**Coverage (derived, not stored).** From tasks' `section_ids` (many-to-many) the daemon computes how well the plan is covered: per section = number of tasks referencing it + their status rollup (none / pending / in_progress / done); plan-level = fraction of sections with ≥1 task, weighted by done. Rendered as a coverage meter on the plan node and a per-section indicator. Computed on read so it never goes stale.

The orchestrator declares `milestone_kinds` **when forming the plan** — this is the set of stages a milestone can be (matching the Figma columns: Status Update → Testing results → Review). The canvas renders declared kinds as aligned lanes/columns across every task's chain, so milestones of the same kind line up vertically. A `Milestone.kind` must reference a declared `key`. An undeclared kind is **not** silently accepted — it returns a teaching error (see §4.3) listing the declared kinds and how to add one (`plan kinds set`, or `--add-kind` to declare-and-use in one call). This keeps the column structure meaningful and trains the agent in-band.

### 3.2 Task
```
Task {
  id: string
  name: string          // codename (e.g. "Dante", "Munch") — generated, human-memorable
  type: string          // FLEXIBLE open string; known hints: implement | research | review | prototype | docs | fix
  model: string | null  // assigned provider+model (e.g. "claude:opus", "cursor:composer-2.5")
  model_rationale: string | null // WHY this model was chosen for this task (routing reasoning)
  status: "pending" | "in_progress" | "blocked" | "done" | "cancelled" // CLOSED enum — logic reasons on this
  status_label: string | null // optional FREE text shown on the card ("waiting on review", "90%"); never drives logic
  section_ids: string[]  // which plan Section(s) this task covers — MANY-TO-MANY (a task may cover several sections; a section may have several tasks). Drives coverage + section↔task edges
  group: string | null   // optional TaskGroup key (≈ phase); null = ungrouped (see 3.8)
  body: string          // markdown — the task card body
  intent: string | null  // orchestrator's extra free-text instruction for the worker, folded into the spawn prompt
  assigned_pane_id: string | null
  peer: string | null    // mesh peer where the assigned tab lives (null = local machine)
  canvas: ElementCanvas // this card's own position / display state (see 3.5)
  milestones: Milestone[] // the horizontal report chain
}
```
`type` is an **open string** by design (no hard-coded enum) — known values are hints for icon/color only. The orchestrator chooses; the system never rejects an unknown type.

### 3.3 Milestone (report node)
```
Milestone {
  id: string
  from: AgentRef        // who reported — AUTO-resolved by the daemon from the caller's tab identity, not hand-typed
  kind: string          // status_update | testing | review | decision | ... (open string)
  text: string          // markdown
  ts: u64
  links: string[]       // ids of next milestone(s); enables chains AND loop-back
  loop_id: string | null  // set if this node belongs to a LoopSpec iteration (see 3.6)
  iteration: number | null // which loop pass produced it (0-based)
  canvas: ElementCanvas // this milestone node's own position / display state (see 3.5)
}
```
Milestones are the persisted projection of `agent report`. A report tagged with `--plan/--task` appends a milestone. `links` carries the chain order and can point backward to form the self-improving loop.

Authorship is **never hand-typed**. The daemon resolves `from` from the calling Tab's env identity (`CODETERM_PANE_ID` → session + title), exactly as `agent report` already does. An optional `--from` flag exists only as a rare manual override (e.g. backfilling on behalf of another agent).

### 3.4 Identity & ownership
Tab ids are ephemeral (regenerated on restart); plans must survive restart and re-attach.

```
OrchestratorRef { pane_id: string|null, session_id: string|null, title: string|null }
AgentRef        { pane_id: string|null, session_id: string|null, title: string|null }
```
- Ownership key = `session_id` when available (stable across restart), else `pane_id`.
- **Owner state (derived):** `owned` (owner Tab live) | `unowned` (no live owner). A plan also stores `owner_history: TransferRecord[]` (`{from, to, ts, reason}`) for audit.
- **Canvas render filter** = `status == active`. An `owned` plan renders in its owner's cluster. When the owner dies, the plan becomes `unowned` — it **leaves the owner's cluster** (this is "disappears from the canvas") but is **not deleted**: it stays active and **claimable**, surfaced via `plan list --unowned` and an optional unowned tray. An orchestrator resuming the same `session_id` **auto-reattaches** (back to `owned`).
- Archived plans are never rendered and not counted.
- **Transfer is first-class** (see §4.6): explicit handoff, voluntary disown, claim. Same-machine orchestrators in v1; cross-machine ownership is deferred with plan replication, though a remote worker still reports home to whoever owns the plan now.

### 3.5 Canvas layout
Every renderable element (the plan's spec card, each task, each milestone) carries its **own** layout co-located on the entity — tasks and milestones are first-class canvas objects, not just children of the plan:
```
ElementCanvas {
  x: number | null      // null → auto-layout decides; a number → manual-drag override
  y: number | null
  collapsed: boolean     // card folded to a compact chip
}
```
- `Section.canvas`, `Task.canvas`, `Milestone.canvas` hold each node's position + display state.
- `Plan.canvas` holds only a viewport hint:
```
Plan.canvas {
  viewport: { panX: number, panY: number, scale: number } | null // last camera, for restore
}
```
Auto-layout is the default (null x/y); manual drags persist as overrides on the element itself. `plan layout set --element <id>` routes to the right entity by namespaced id (`section:<id>`, `task:<id>`, `ms:<id>`, `el:<id>`).

### 3.6 Loop (self-improving cycle) — optional
A general abstraction for repeated multi-agent cycles. **Entirely optional** — a plan may have no loops, and most tasks are not loops; this is a capability, not a requirement. It generalizes the canonical example "improve a code section → review → scout for what else to improve → repeat ×N".
```
LoopSpec {
  id: string
  target: string         // free description/ref of what the cycle operates on
  stages: LoopStage[]     // ordered roles run each iteration
  budget: number          // max iterations (e.g. 25)
  count: number           // iterations done
  stop_when: string | null // optional early-exit signal ("scout finds no further improvements")
  status: "running" | "done" | "exhausted" | "stopped"
}
LoopStage { key: string, role: string, model: string | null, why: string | null, prompt: string | null }
```
- Each **iteration** runs `stages` in order; every stage is an agent producing one `Milestone` tagged with this `loop_id` + `iteration`.
- The canvas renders iterations as repeated chain segments (a coil/looped visual), stages aligning to declared `milestone_kinds`.
- **Termination:** `count >= budget` (the loop-budget guard, §4.4) **OR** a stage reports that `stop_when` is satisfied → `status = done`. Whichever comes first.
- Lightweight case: `plan task add --loop-from <task_id>` is the manual single continuation — it attaches to an implicit default loop on the plan and still counts against a budget. The structured `LoopSpec` is for declared, daemon-driven cycles.

### 3.7 Storage & index

**One storage, no database.** Plans are plain JSON files in the project; there is no sqlite.

- **Source of truth (the only persistence):** `<project>/.codeterm/plans/<plan_id>.json`. Per-**project** `.codeterm/` is new (today only global `~/.codeterm` exists). Project root resolved as git toplevel of the workspace cwd, fallback = workspace cwd. Directory created on demand. (`.codeterm/` is gitignore-friendly; not committed.)
- **Archival is a flag, not a move.** Archiving sets `status: "archived"` inside the same file; the file stays exactly where it is. Active and archived plans coexist in `.codeterm/plans/`. The canvas filters by `status == active`; archived → hidden, never moved or deleted. Un-archive = set `status: "active"`.
- **Index is in-memory only.** At startup the daemon scans `.codeterm/plans/` for the project paths it knows from the open workspaces, builds an in-memory index `{ plan_id → (file_path, owner, title, status, updated_at) }`, and maintains it on every write. It is a derived cache, never the source of truth — wipe it and a rescan rebuilds it. It exists solely so the workspace-overview canvas can list plans across several projects fast, without re-reading every file.
- **Single writer:** the **daemon** owns all writes. Workers/orchestrators never write the JSON directly — they call `codeterm plan ...` → socket/REST → daemon, which serializes writes per-plan with a mutex and does atomic temp-write + rename. This removes the concurrent-section-close race by construction.
- **External edits reconciled:** the files stay hand-editable. The daemon watches `.codeterm/plans/` and, on an external change, reloads the plan, refreshes the in-memory index, and emits `plan-changed` so the canvas reflects a manual edit live. Daemon writes use atomic rename to avoid racing a hand-edit mid-flush.

### 3.8 Provenance — emergent from nodes + edges (NOT a new subsystem)
The whole canvas is one small primitive set — **typed nodes** (each a `CanvasElement`-kind: plan / section / task / milestone / note / decision / …, see §6.3) **+ edges** (a node's `links` ids, drawn as connectors). Different `kind` + different link topology give different "features", reused emergently. Provenance is just more of the same — **no diff/revision-history engine** (explicitly out of scope).
- **Actions / decisions / changes on a document are NODES, not a log field.** "This section was re-scoped", "plan re-planned after the verification pass", a routing decision — each is a small node (a `milestone`/`decision`/`note` element) whose `links` **edge-connect it to the document node** (section / plan / task) it concerns. The "history on the spec" emerges from those edges; there is no `Plan.milestones` / `Section.milestones` array.
- **Same edge mechanism everywhere:** a task→milestone chain, a section→task link, a milestone loop-back, a decision→section note, and a plan→plan relation are ALL the universal `links` edge — the renderer just dispatches by node kind.
- **Inter-plan links:** `Plan.links` is the plan-frame-to-plan-frame edge — a plan discovered/derived while building another links back to its parent.
- **Task groups:** `Plan.task_groups` + `Task.group` are the one non-graph addition — a lightweight grouping attribute (≈ phases), declared like `milestone_kinds`, rendered as labeled bands/clusters; ungrouped tasks are fine.
This keeps the canvas a "film" of the work via the primitives it already has — kind-typed nodes + `links` edges + the §6.3 renderer registry. **Phase 3+ enhancement** (not in read-only v1).

### 3.9 Live mutability & context-resilient editing
The plan/canvas is a **live object edited WHILE work runs** — not a write-once artifact. Requirements evolve mid-execution (sections re-scoped, tasks added, a Phase-2 task amended while Phase-2 workers are mid-flight — exactly how this very plan was built). The design makes that safe and easy:
- **In-flight mutation:** every `codeterm plan ...` handle mutates a running plan; the daemon serializes writes (single-writer, §3.7) and fans out `plan-changed` so the canvas + every consumer live-update (§4 real-time). No stop-the-world — add / change / close while agents work.
- **Easy find-and-change:** read current state with `plan get <id>` (markdown), discover handles via `plan --help` / `codeterm docs ORCHESTRATION`, change one thing with a single targeted command (`plan task update`, `plan section update`, …). Dig in, change, done.
- **Context-resilient (load-bearing):** an agent whose context was compacted or scrolled off still edits safely — it re-orients by re-reading current state (`plan get`), the commands self-document, and teaching errors (§4.3) + adherence reminders (§4.2, with the "if you forgot, re-read X" pointer) catch drift. You never need the whole history in context to change the plan correctly: **read current → change via guided commands.** Same discipline as the orchestrator's self-recovery heartbeat, made intrinsic to plan editing.

## 4. Backend

New Rust surfaces (each file < 1000 lines, split by responsibility):

- `src-tauri/src/plan/mod.rs` + `types.rs` — `Plan`, `Task`, `Milestone`, refs, enums, (de)serialization.
- `src-tauri/src/service/plan/` — `PlanStore` (load/save/list, per-plan mutex, atomic write, index maintenance), project-root resolver, owner-liveness join against the multiplexer.
- `src-tauri/src/api/plan_routes.rs` — REST handlers; register in `src-tauri/src/api/routes.rs`.
- Socket methods `plan.*` for the CLI (same pattern as `agent.report`).
- **Report→milestone linkage:** extend `ReportBody` + `submit_report` (`agent_routes.rs` / `hierarchy.rs`) to accept optional `plan_id` + `task_id`; on report, append a `Milestone` to that task and persist. Reports without plan tags behave exactly as today (no regression).
- **Real-time (mandatory):** every mutation emits `plan-changed` to **both** transports — the native Tauri event **and** the websocket feed — carrying the changed `plan_id` (+ minimal delta). Native app and WebUI both live-update with **no polling**; the canvas is purely subscription-driven, mirroring `pane-status-changed` / `mem-activity`. Any write through any path (CLI, REST, daemon) must fan out this event.

### 4.1 Task binding via env
When a task is assigned to a Tab (`plan task assign`, or a worker spawned for a task), the daemon exports `CODETERM_PLAN_ID` and `CODETERM_TASK_ID` into that Tab (alongside the existing `CODETERM_*` vars). Workers then report against their task without being told the ids: `codeterm agent report --plan $CODETERM_PLAN_ID --task $CODETERM_TASK_ID ...`. Creating a plan exports `CODETERM_PLAN_ID` into the orchestrator's own Tab too.

### 4.2 Plan-adherence reminders
Agents forget they are inside a plan and stop reporting. The daemon nudges them — smartly, not on a dumb timer.

- Per Tab bound to an **active** task/plan, track `turns_since_plan_touch`, incremented on each agent turn.
- **Reset to 0** whenever that Tab runs any `codeterm plan ...` or `codeterm agent report --plan ...`. A diligent agent is therefore never nagged; only a silent one is.
- When the counter crosses `nudge_after_turns` (default 12), inject a system reminder into the Tab via the existing report/reminder injection path. After each nudge the interval grows ×1.5 (capped) to avoid spam.
- Differentiated copy for Worker vs Orchestrator (see §7). Escalation: after several ignored nudges the final one is blunt ("UNREPORTED task; your parent is blind until you report").
- **Two trigger modes, both config-driven:** (a) **message-count** — the `turns_since_plan_touch` threshold above; (b) **time interval** — an active plan also pings its **owner orchestrator** every `nudge_interval_secs` of wall-clock (default 1800 = 30 min), independent of message activity. Either trigger fires a nudge; both reset on a plan-touch.
- **This replaces the orchestrator's manual `--delay` self-heartbeats:** once a plan exists, the daemon's plan-adherence pinger IS the heartbeat — the orchestrator no longer hand-schedules delayed self-messages; the active plan periodically pings it (time + count) with the self-recovery pointer baked in.
- Config: `[orchestration]` in `config.toml` — `plan_reminders = true|false`, `nudge_after_turns = 12`, `nudge_interval_secs = 1800`. CodeTerm-internal config: defaults ship in code, the human tunes their own cadence (per-instance, so dev/prod can differ); agents do not need to edit it.

### 4.3 Teaching errors (self-correcting CLI)
The second in-band training channel (the first is §4.2 reminders): every validation failure in `codeterm plan ...` / `codeterm agent report` returns an **actionable, teaching** message on stdout — what was wrong, the valid options, and the exact command to fix — never a bare "invalid argument". The agent reads it inline and self-corrects. Required cases:
- **Undeclared milestone kind** → list the plan's declared kinds; offer `plan kinds set` or `--add-kind` (declare-and-use).
- **Invalid task status** → list the closed enum; point free-text nuance to `--status-label`.
- **Unknown plan/task id** → say so + `codeterm plan list --active`.
- **Missing required field** → print the minimal correct invocation.
- **Dash-leading free text** (the clap pitfall) → hint the `--field=value` / `--field-file` form (reuse the mem-write lesson).
- **Loop budget exhausted** → `✗ loop budget 3/3 reached for plan "..."; extend with codeterm plan loop set --budget N, or close the plan.`
Errors are concise, lead with `✗`, and always end with a runnable fix line.

### 4.4 Loop bound
Loops (§3.6) are budgeted, not infinite — when used at all (they are optional). For a declared `LoopSpec`, the daemon drives iterations: each pass runs the stages, increments `count`, and stops when `count >= budget` **or** a stage signals `stop_when` is satisfied (→ `status = done`). For the lightweight `--loop-from` case, a continuation consumes one budget unit of the plan's implicit loop. On budget exhaustion the daemon refuses the next iteration/continuation with the §4.3 teaching error and nudges the orchestrator to extend (`plan loop create/stop`) or close. This is the loop-until-budget guard so a self-improving plan can't churn forever; `budget` is mandatory on a declared loop, the implicit loop defaults to unbounded until set.

### 4.5 Task spawn prompt assembly
When a worker is spawned for a task (assign + spawn), the daemon **auto-assembles** its prompt from:
1. The standard worker role suffix (already exists: "you are a Worker… report back…").
2. **Task framing:** `You are assigned task "<name>" (<type>) in plan "<plan title>". Task: <task.body>`.
3. **Scope restriction:** the titles/ids of the task's `section_ids` + `You MAY read the whole plan (codeterm plan get $CODETERM_PLAN_ID), but your work is RESTRICTED to the section(s) above — do not wander outside them.`
4. **Orchestrator intent:** `task.intent` verbatim, if set (the extra instruction the orchestrator wrote for this worker).
5. The task-binding env (§4.1) so reporting needs no ids.
The worker thus gets exactly its slice, with the full plan readable but bounded, plus the orchestrator's custom intent — without the orchestrator hand-writing the whole prompt.

### 4.6 Ownership transfer
Plans change hands; ownership is first-class, not frozen at creation. All cases:
- **Explicit handoff** — `plan transfer <plan_id> --to <orchestrator>` (target = Tab id, session id, or "current focused orchestrator"). Owner ref + `owner_history` updated; the plan re-clusters under the new owner on the canvas (live via `plan-changed`).
- **Voluntary disown** — `plan disown <plan_id>` → `unowned`, leaves the cluster, stays active and claimable.
- **Owner death** — the owner Tab dies → derived `unowned`; not deleted, claimable (per §3.4).
- **Same-session resume** — owner restarts with the same `session_id` → auto-reattach to `owned`.
- **Claim** — `plan claim <plan_id>` → the caller's orchestrator takes an `unowned`/stale plan.
- **Report re-routing (the load-bearing rule):** a task-tagged report/close resolves to the plan's **current owner at delivery time** (lookup, never a frozen target). So transferring a plan mid-flight re-points every in-flight worker — local or remote-over-mesh — to the new owner with no worker change. The existing report target priority (`parent` → `report_target` → `spawned_by`) is overridden by the plan→current-owner lookup when a report carries `--plan`.
- Every transition writes `owner_history` and emits `plan-changed`.

### 4.7 Mesh task operations (plan stays local)
The plan is **not** replicated across the mesh — one orchestrator owns it locally (the §2 decision holds). Mesh is used only as a **transport at the task level**, reusing the existing mesh report routing:
- **Remote assignment/spawn:** `--mesh <peer>` (alias `--peer`) on task commands targets a Tab on another machine. For a spawn, the local daemon **forms the spawn command and sends it over mesh** to create the tab on the remote peer, binding `CODETERM_PLAN_ID`/`TASK_ID` there. The task records `peer`.
- **Remote report/close:** a worker on another machine runs `codeterm agent report --plan --task ...` / `plan task close ...`; the report is addressed to the owning orchestrator's identity and **mesh delivers it back** to the local orchestrator, which applies the milestone/close to the local plan file. So others can advance and close tasks remotely without the plan ever syncing.
- This emergently extends the **local** canvas across the whole network: remote Tabs appear as tasks with a `peer` badge; their milestones flow home via mesh. No plan CRDT, no replication — just task-scoped messages over the existing mesh.

### REST routes
Representative set — every CLI handle in §5 has a matching route (section/loop/transfer/milestone-update/note/element/layout mirror their `plan ...` subcommand). Core:
```
POST   /api/plans                         create
GET    /api/plans                         list  (?project=&workspace=&status=&orchestrator=)
GET    /api/plans/{id}                    get
PATCH  /api/plans/{id}                    update title/status (sections via the section routes)
DELETE /api/plans/{id}                    delete
POST   /api/plans/{id}/archive            archive
POST   /api/plans/{id}/tasks              task add
PATCH  /api/plans/{id}/tasks/{tid}        task update
POST   /api/plans/{id}/tasks/{tid}/close  worker closes section (status=done + optional result milestone)
POST   /api/plans/{id}/tasks/{tid}/assign assign tab
POST   /api/plans/{id}/tasks/{tid}/milestones  milestone add
PATCH  /api/plans/{id}/layout             set element position(s)
```

## 5. CLI handles — `codeterm plan ...`

Thin wrappers over the routes (pattern: `cli/src/args/plan.rs`, `cli/src/cmd/plan.rs`, register in `args/cli.rs` + `main.rs`). All free-text args use clap `allow_hyphen_values` and support `--<field>-file` / stdin (reuse the mem-write robustness pattern). All reads support `--json`.

```
codeterm plan create  --title <t> [--spec <md>|--spec-file <p>] [--kinds status_update,testing,review] [--loops <n>] [--project <path>]
codeterm plan section add    <plan_id> --title "..." [--body/--body-file ...] [--parent <section_id>]
codeterm plan section update <plan_id> <section_id> [--title|--body/--body-file ...]
codeterm plan section rm     <plan_id> <section_id>
codeterm plan kinds set <plan_id> --kinds status_update,testing,review   # redefine the chain's stage columns
codeterm plan loop create <plan_id> --target "<desc>" --budget 25 \
   --stage improve:model=claude:opus --stage review:model=claude:sonnet --stage scout:model=opencode \
   --stop-when "scout finds no further improvements"               # declared self-improving cycle (optional)
codeterm plan loop set    <loop_id> [--budget <n>] [--stop-when "<cond>"]   # extend/adjust an existing loop
codeterm plan loop status <loop_id>
codeterm plan loop stop   <loop_id>
codeterm plan list    [--project <path>] [--workspace] [--active|--archived|--all] [--orchestrator <id>] [--json]
codeterm plan get     <plan_id> [--json]            # markdown render by default
codeterm plan update  <plan_id> [--title <t>] [--spec/--spec-file ...] [--status active|archived]
codeterm plan archive <plan_id>
codeterm plan delete  <plan_id>
codeterm plan transfer <plan_id> --to <orchestrator>   # tab id | session id | "focused" — hand ownership over
codeterm plan disown   <plan_id>                       # release ownership → unowned/claimable
codeterm plan claim    <plan_id>                       # caller's orchestrator takes an unowned/stale plan
codeterm plan list --unowned                           # plans with no live owner, claimable

codeterm plan task add    <plan_id> --name <n> --type <kind> [--sections <id,id,...>] [--model <m>] [--why <text>|--why-file <p>] [--body/--body-file ...] [--intent <text>|--intent-file <p>] [--assign-tab <id>] [--loop-from <task_id>]
codeterm plan task update <plan_id> <task_id> [--name|--type|--model|--why|--status|--status-label|--body ...]
codeterm plan task close  <plan_id> <task_id> [--result/--result-file ...] [--mesh <peer>]   # worker closes their section; remote close routes home via mesh
codeterm plan task assign <plan_id> <task_id> --tab <id> [--mesh <peer>]   # --mesh/--peer: spawn/target on another machine, daemon brokers over mesh
codeterm plan task rm     <plan_id> <task_id>

codeterm plan milestone add    <plan_id> <task_id> --kind <k> [--add-kind] [--text/--text-file ...] [--from <agent>]
codeterm plan milestone update <plan_id> <task_id> <ms_id> [--kind|--text/--text-file ...]
codeterm plan milestone rm     <plan_id> <task_id> <ms_id>
codeterm plan layout set    <plan_id> --element <id> --x <n> --y <n>
codeterm plan note add      <plan_id> [--text/--text-file ...] [--link-task <id>] [--link-section <id>] [--at <x,y>|--near-focus]
codeterm plan element add   <plan_id> --kind <k> [--content/--content-file ...] [--link <id,...>] [--at <x,y>|--near-focus]  # generic extension point
```
Plus the report linkage: `codeterm agent report --plan <id> --task <tid> "<msg>"` auto-appends a milestone.

Owner of a created plan = the caller's orchestrator identity, derived from `CODETERM_PANE_ID` / `CODETERM_ROLE` env (already exported at spawn).

## 6. Frontend

New module `src/components/Canvas/`:

- `useCanvasViewport.ts` — shared pan/zoom hook extracted from `MeshCanvas` (middle-mouse drag pan, wheel zoom, fit-to-view, transform state). **Retrofit `MeshCanvas` onto it** (targeted cleanup; removes the duplicate transform code).
- `CanvasView.tsx` — top-level; takes a `mode`:
  - `mode="workspace"` → all **active** plans whose owner orchestrator is live in this workspace.
  - `mode="focus"` → plans of a specific orchestrator (and its subordinates), opened from the Plan button.
- Cards (absolutely-positioned DOM, markdown via the existing chat-ui renderer): `SpecCard`, `TaskCard` (marker + name + model chip + status badge + body), `MilestoneChain` + `MilestoneCard`.
- `layout.ts` — cascade auto-layout: Spec left, tasks stacked, milestones as a horizontal chain per task; bounds-based collision avoidance; manual-drag overrides persisted via `plan layout set`.
- `usePlans.ts` — REST fetch + subscribe to `plan-changed`; live updates.
- **`CanvasComposer.tsx` — talk-to-area input.** A floating, small centered/bottom bubble (not full-screen), styled like the chat-UI composer, with text + voice + document-attach (reuse the existing composer + attachments + voice machinery). It is **context-addressed**: whatever you type/say is routed to the orchestrator that owns the region currently in focus.
  - **Targeting:** in `mode="focus"` → that plan's owner orchestrator. In `mode="workspace"` → the orchestrator whose region the viewport is centered over (or an explicitly selected plan/area); a small target chip on the bubble shows "→ <orchestrator/plan>" so it's never ambiguous.
  - **Context attached:** the message carries which plan/task/area is in view, so the orchestrator knows what you're reacting to.
  - **Delivery:** reuse the existing send path — message + attachments go to that orchestrator's tab (the same mechanism `codeterm send --tab <id>` / the chat composer already use). No new transport.
  - Always present whenever a region is in view → the canvas is conversational, not read-only.

Mounts:
- **Workspace empty-state:** in `App.tsx` `<main>` when `mainView === "workspace"` and a workspace is active but no tab is focused → render `<CanvasView mode="workspace">`. This is the 2D playground; later phase adds spawn-orchestrator / hand-task affordances on empty canvas space.
- **Plan button:** in `PaneTopBar.tsx` right of `PaneModeSwitch` → `<PlanButton>`. On an **orchestrator** Tab it shows when this Tab (self or ancestor via `parent_pane_id`) owns an active plan with open tasks → click opens `<CanvasView mode="focus">` on that plan. On a **worker** Tab bound to a task (`CODETERM_PLAN_ID`/`TASK_ID` set) it shows too → click opens the plan focused on this worker's task. **Change pulse:** on a `plan-changed` for the plan(s) it represents (new milestone, status, task…), the button itself **pulses** ("bam — change"), not a dot badge; the pulse clears once the user opens the canvas.

Reuse note: Swarm's report-particle animation can later animate milestone chains; not v1-critical.

### 6.1 Rendering & layout algorithm
Canvas content is a nested cascade, laid out without overlap, stable across live updates, navigable by zoom, and **spatially addressable** (position decides who you talk to).

**Nested regions.**
- Each **plan** is a bounded frame (Figma-like) that auto-sizes to its contents: the section cascade (left) → task rows → milestone columns. The frame's AABB is the plan's region. The plan node shows a **coverage meter** (derived from tasks per section).
- The **spec is a portyanka**: sections/subsections render as a connected vertical stack of nodes down the left side.
- Edges are **plain connecting lines, not arrows** — direction is implicit in the left→right arrangement (plan → tasks → chains), so no arrowheads are drawn. Section↔task is **many-to-many**: a section connects to every task covering it, and a task draws edges back to each of its `section_ids`. A section with no tasks reads visually as uncovered.
- **Left→right bands:** plan/sections (left) → tasks (middle, rendered a touch smaller than the plan frame, hanging off their section parts) → milestone chains (right).
- **Milestones as nodes/groups:** milestones render as circles (or content inside circles); within a task they group by `iteration` into clusters. Node size, group size, and task size are all tunable (§6.2).
- **Chains must use vertical space — never run to infinity.** A milestone chain does not extend rightward forever: it is **width-bounded** and wraps/clusters using the up-down area. Within one iteration, milestones snap into the declared `milestone_kinds` lanes (Status Update | Testing | Review), so the chain spans a fixed number of columns, not unbounded width. Across many nodes/iterations it folds into stacked rows or a compact **cluster ("cloud")** with order preserved by the connecting edges. The layout caps horizontal extent and spends the vertical budget first.
- **Loops** render as folded repeated segments (a coil) with an `iteration` badge; loop-back `links` are return edges — explicitly so 25 iterations stack vertically/cluster rather than streaming off-screen to the right.
- Plans owned by the **same orchestrator** cluster adjacently under a soft owner label; different orchestrators → separate clusters.

**Layout pipeline (two phases — stable + incremental).**
1. *Intra-plan*: compute spec/task/milestone positions inside each frame → the frame bbox. Manual `canvas.x/y` overrides win; null → computed. Adding a task/milestone **grows the frame in place** — no global reshuffle, so the canvas doesn't jump on every `plan-changed`.
2. *Region packing*: place plan frames in the workspace via bin-packing (sorted owner → `updated_at`), AABB collision avoidance + padding, expanding world bounds as needed. Deterministic: same data → same layout.

**Zoom / LOD** (reuse the memory-canvas ratio thresholds):
- far → plan frames as summary tiles (title, task count, status rollup); milestones hidden.
- mid → task cards; milestones collapse to chips.
- near → full milestone markdown.

**Focus / region detection (this is what makes talk-to-area meaningful).**
- On viewport change, compute the viewport **center** in canvas coords.
- The plan frame whose AABB contains / is nearest the center = the **focused plan** → its owner orchestrator is the `CanvasComposer` target (shown in the chip).
- Within that frame, the task/milestone cards intersecting the central focus zone = the **in-view documents**.
- When you type/speak, the daemon auto-attaches the focused plan + the in-view task/milestone ids (and their text) as context to the message routed to that orchestrator. "Talking to the area" = your words **plus exactly the docs under your viewport center**. Moving the camera changes who you address and what context attaches — that is the meaning of navigation here.

**Persistence.** Computed positions are ephemeral; only manual overrides (`ElementCanvas.x/y`, `collapsed`) and the last `Plan.canvas.viewport` persist. A fresh client recomputes the rest deterministically.

**Virtualized rendering.** Only elements intersecting the viewport (plus a margin) are mounted; off-screen nodes are culled, reusing the memory-canvas viewport-culling approach. Loops ×N iterations and large plans stay performant — node count on screen is bounded by the viewport, not the data.

**No magic numbers.** Every constant the layout uses comes from the settings object in §6.2 — none are hardcoded inline.

### 6.2 Layout settings (tunable, real-time, no recompile)
Every layout parameter lives in one typed abstraction, not scattered as inline magic numbers, so the whole canvas can be tuned live and frozen later.
```

CanvasLayoutSettings {
  cardWidth, cardMinHeight, cardMaxHeight,
  sectionGapY, taskGapY, laneWidth, laneGapX,
  framePadding, frameGap,            // region packing
  chainWrapWidth, clusterRadius,     // chain folding / clouds
  collisionPadding,
  zoomFarThreshold, zoomNearThreshold,
  edgeCurve,
  taskScale, milestoneNodeScale, milestoneGroupScale, // per-element sizing
  layoutStrategy,                    // SWAPPABLE principle: "lanes" | "cluster" | "radial" | ...
  ...
}
```
- **The principle is swappable, not just the numbers.** `layoutStrategy` selects HOW elements populate (lane columns vs. clustered clouds vs. radial), so we can try different arrangements at runtime, not only nudge sizes. Each strategy reads the same numeric knobs.
- **Single source:** the layout algorithm (§6.1) reads ALL constants from this object. Adding a knob = a field here, never a literal in code.
- **Real-time tuning:** a **Canvas debug panel** renders a control per field. Editing a value updates the settings in memory and **re-runs the layout immediately** — no recompile, no rebuild. We tune sizes/gaps/thresholds together by dragging sliders and watching the canvas re-flow live.
- **Lifecycle:** defaults ship in code; the debug panel overrides at runtime; a "save" persists to `config.toml [canvas]`. Tuned values can later be baked as defaults or left exposed as user-facing canvas settings.
- Debug panel is dev/toggle-gated tooling (Phase 2/3), but the settings abstraction itself lands with the layout in Phase 2 so nothing is ever hardcoded.

### 6.3 Extensible canvas elements
Every node on the canvas shares a common base — the canvas core positions/connects "physical" boxes and **dispatches rendering by kind through a registry**, so new content types plug in without touching the layout engine.
```
CanvasElement {
  id: string
  kind: string          // "plan" | "section" | "task" | "milestone" | "note" | "html" | "prototype" | "checklist" | ...
  canvas: ElementCanvas  // position / display state
  content: any          // markdown (default) or kind-specific structured payload
  links: string[]        // ids of tasks/sections/other elements this attaches to
}
```
- **Built-in kinds:** plan/section/task/milestone (the structured model above) are themselves elements of these kinds. Free-form kinds (`note`, `html`, `prototype`, `checklist`) live in `Plan.elements`.
- **Renderer registry:** a `kind → renderer` map. Adding a type = register a renderer (+ optional payload schema); the canvas core, layout, pan/zoom, and focus-detection are unchanged. Unknown kind → safe fallback card. HTML/interactive kinds render sandboxed.
- **Markdown is the default content**; richer kinds (checkboxes/task-lists, HTML, UI mockups) extend from there.
- **Linkable:** any element `links` to tasks/sections — "tie this mockup to these tasks", "attach this note to that task".
- **Notes ship in v1:** a `note` element — free markdown, positionable, linkable to a task/section. An agent can spawn one in the area you're viewing.
- **Agent-spawned, real-time, in view:** an agent (via the API/CLI) can create an element into a target region (explicit x/y, or "near the current focus"), and it renders live via the `plan-changed` feed where the user is looking. Example future flow: "draw ABC interface here" → agent emits a `prototype` element in that area, then "tie it to these tasks" → sets its `links`.

## 7. Prompts & docs

Separation the user insisted on: **mechanics (toolset) vs principle (how to orchestrate)**.

- **New `src-tauri/prompts/orchestration_toolset.md`** — the cascade-expandable **toolset reference**: the `codeterm plan ...` handles and how Plan/Task/Milestone/Canvas relate. Mechanics only.
- **`codeterm_cli.md`** — add a short "Plans & Canvas" pointer that references the toolset doc + `codeterm plan --help`; do **not** inline the full toolset (keeps the CLI prompt lean).
- **Orchestration principle** (when to spawn, model routing, hygiene) stays in the mem skill `codeterm-orchestrator` — runtime-updatable, possibly a plugin later. Untouched by this feature except a one-line pointer to the toolset.
- Doc updates: new `docs/CANVAS.md` (overview + data model + CLI), `docs/API.md` (routes + CLI), `docs/PROMPTS.md` (new prompt + pointer), `docs/CONFIG.md` if any config knob is added. Bump each `<!-- revision -->`.

## 8. Phasing

Spec covers everything; the implementation plan sequences it:

- **Phase 1 — Backend core.** Data model, `PlanStore`, per-project storage + index, `codeterm plan` CLI, REST/socket, `report→milestone` linkage, `plan-changed` events. Unit tests; a `evals/` bench for the CLI handles + report linkage.
- **Phase 2 — Read-only Canvas.** `useCanvasViewport` (+ Mesh retrofit), `CanvasView`, cards, auto-layout, workspace empty-state mount, Plan button, live updates.
- **Phase 3 — Interactivity.** Pan/zoom polish, manual drag + layout persistence, the **talk-to-area `CanvasComposer`** (context-addressed input → owning orchestrator), spawn-orchestrator / assign-task from empty canvas, archive UX.
- **Phase 4 — Prompts/docs/toolset.** `orchestration_toolset.md`, `codeterm_cli.md` pointer, `docs/CANVAS.md`, skill pointer, orchestration-toolset eval.

### 8.1 Testing & evals
- **Rust unit:** `PlanStore` (atomic write, per-plan mutex, index rebuild, external-edit reload), section-tree ops, coverage computation, loop budget/stop, report→milestone linkage.
- **CLI eval** (`evals/plan-lifecycle/`, real daemon): drive the full lifecycle through `codeterm plan ...` — create → sections (auto-split) → tasks (many-to-many) → assign+spawn → report→milestone → close → loop budget → archive — and assert teaching errors fire on bad input. Mirrors the `cli-agent` bench shape.
- **Frontend:** layout determinism (same data → same boxes, no overlap), focus/region detection (center → correct plan + in-view docs), virtualization (off-screen culled), realtime (`plan-changed` → live update) via Playwright web-mode.

## 9. Deferred / open

- **Full plan replication / CRDT across peers** — deferred (not needed). NOTE: task-level mesh ops (remote spawn/report/close, §4.7) are **in** v1; only whole-plan sync is deferred. Model stays mesh-ready: stable ids, owner identity, `updated_at`.
- **Big-plan bounds expansion / overlap solver** — basic collision avoidance now, refine later.
- **Progress visualization** — schematic in the mockups; defer the fancy version.

## 10. Build allocation (agents/models)

- **Phase 1 (Rust):** split (a) types+store+storage+index, (b) CLI handles, (c) REST/socket + report linkage. `opus`/`sonnet` for store/concurrency; `cursor:composer-2.5` or `codex` for mechanical CLI/route wiring.
- **Phase 2 (Frontend):** `cursor:composer-2.5` for card components/CSS; `opus`/`sonnet` for the viewport hook + auto-layout/collision.
- **Research/mechanical:** `opencode`/`codex` (cheaper) for isolated investigations.
- **Reviews:** `opus` code-review pass per phase.
- Isolation: worktree per worker; orchestrator integrates into the warm checkout and runs incremental `./build.sh check`.
```

<!-- revision: 2 -->
