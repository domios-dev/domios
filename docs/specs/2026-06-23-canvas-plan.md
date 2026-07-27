# Canvas Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Canvas — a persistent, per-workspace plan object (Spec/sections → tasks → milestone chains → loops) edited via `codeterm plan` CLI handles and rendered as a live Figma-like 2D board.

**Architecture:** Plans persist as one JSON file per plan under `<project>/.codeterm/plans/`; the daemon is the sole writer (per-plan mutex + atomic rename) with an in-memory index and a file-watcher for hand-edits. Worker `agent report --plan --task` appends milestones; every mutation fans out a `plan-changed` event to native + websocket. Frontend reuses the Mesh DOM+CSS-transform pan/zoom (extracted into a shared hook) with absolutely-positioned markdown cards and a deterministic, virtualized, tunable layout engine.

**Tech Stack:** Rust (Tauri v2, axum, tokio, serde, parking_lot, notify) backend; clap CLI; React/TypeScript frontend; existing report/hierarchy/mesh subsystems.

**Spec:** `docs/specs/2026-06-23-canvas-design.md` (read it first — this plan implements it).

---

## Phasing & how to use this plan

- **Phase 1 — Backend core** is written below in full TDD detail. It ships a working, testable subsystem with NO UI: create/edit/read plans, tasks, milestones, loops via `codeterm plan`, report→milestone linkage, events. **Build this first.**
- **Phases 2–4** are task-mapped (files, responsibility, gates, dependencies, model allocation). At the start of each, re-invoke `superpowers:writing-plans` to expand its map into bite-sized TDD tasks against the then-current code. Do NOT pre-pseudocode the React layout engine here — it is tuned empirically (§6.2 debug panel).

## Repo gates (every task)

- Rust: `cargo fmt` then `./build.sh check` (warm, ~6–22s). Narrowed `cargo test <path>` capped 90s.
- Each source file < 1000 lines (`scripts/check-file-sizes.sh`). Split by responsibility.
- English only in code/comments/commits. 2-line comment cap. No commit/push without explicit user request.
- Lock order (if touching shared state): mux → output_buf → screen_state → agent_tracker → pty. parking_lot non-reentrant.

---

## File structure (all phases)

**Phase 1 — backend**
- Create `src-tauri/src/plan/mod.rs` — module facade (<800 lines).
- Create `src-tauri/src/plan/types.rs` — all data types + enums + serde.
- Create `src-tauri/src/plan/coverage.rs` — pure coverage computation.
- Create `src-tauri/src/plan/loop_engine.rs` — loop budget/iteration/stop logic (pure where possible).
- Create `src-tauri/src/service/plan/mod.rs` — `PlanStore` (load/save/list, mutex, atomic write, index).
- Create `src-tauri/src/service/plan/paths.rs` — project-root resolver, `.codeterm/plans/` dir.
- Create `src-tauri/src/service/plan/watcher.rs` — file-watcher → reload + `plan-changed`.
- Create `src-tauri/src/service/plan/reminders.rs` — plan-adherence nudge counter.
- Create `src-tauri/src/api/plan_routes.rs` — REST handlers.
- Modify `src-tauri/src/api/routes.rs` — register `/api/plans/*`.
- Create `src-tauri/src/socket/server/plan.rs` — JSON-RPC `plan.*` socket dispatch (CLI bridge; copy `agent.rs`).
- Modify `src-tauri/src/socket/server/mod.rs` — register `"plan"` prefix + add `plan.*` to `system.capabilities`.
- Modify config schema (`grep -rn "pub struct .*Config" src-tauri/src/config`) — add `OrchestrationConfig { plan_reminders, nudge_after_turns }`.
- Modify `src-tauri/prompts/codeterm_cli.md` — minimal `codeterm plan ...` lines so `prompt_cli_commands_exist` passes (full prose in Phase 4).
- Modify `src-tauri/src/api/agent_routes.rs` — `ReportBody` gains `plan_id`, `task_id`.
- Modify `src-tauri/src/service/hierarchy.rs` — `submit_report` appends milestone + plan→current-owner routing.
- Modify the PTY spawn lifecycle (`src-tauri/src/service/pty_io/lifecycle.rs`) — export `CODETERM_PLAN_ID`/`CODETERM_TASK_ID`.
- Modify `src-tauri/src/lib.rs` / module root — declare `mod plan;`.
- CLI: create `cli/src/args/plan.rs`, `cli/src/cmd/plan.rs`; modify `cli/src/args/cli.rs`, `cli/src/main.rs`, `cli/src/cmd/manifest.rs`.
- Eval: create `evals/plan-lifecycle/` (README + run.sh).

**Phase 2 — read-only canvas**
- Create `src/components/Canvas/useCanvasViewport.ts`, `CanvasView.tsx`, `cards/*`, `layout/*`, `usePlans.ts`, `settings.ts`.
- Modify `src/components/MeshPanel/MeshCanvas.tsx` — retrofit onto `useCanvasViewport`.
- Modify `src/App.tsx` — mount `<CanvasView mode="workspace">` in workspace empty-state.
- Modify `src/components/Terminal/PaneTopBar.tsx` — mount `<PlanButton>`.

**Phase 3 — interactivity** — `CanvasComposer.tsx`, drag/layout-persist, focus-detection wiring, spawn-from-canvas, debug panel.

**Phase 4 — prompts/docs** — `src-tauri/prompts/orchestration_toolset.md`, edits to `codeterm_cli.md` / `orchestrator_suffix.md` / `worker_suffix.md`, `docs/CANVAS.md`, mem-skill pointer.

---

## PHASE 1 — Backend core

### Task 1: Plan data types

**Files:**
- Create: `src-tauri/src/plan/mod.rs`
- Create: `src-tauri/src/plan/types.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod plan;` near the other `mod` decls)
- Test: inline `#[cfg(test)]` in `types.rs` (move to `types/tests.rs` if it grows past the file budget)

- [ ] **Step 1: Write the failing test** — serde round-trip + defaults.

```rust
// in src-tauri/src/plan/types.rs, #[cfg(test)] mod tests
#[test]
fn plan_roundtrips_and_defaults() {
    let p = Plan::new("Voice UX".into(), "/repo".into(), OrchestratorRef::from_pane("abc123"));
    assert_eq!(p.status, PlanStatus::Active);
    assert!(p.sections.is_empty() && p.tasks.is_empty() && p.loops.is_empty());
    let json = serde_json::to_string(&p).unwrap();
    let back: Plan = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, p.id);
    assert_eq!(back.title, "Voice UX");
}

#[test]
fn task_status_is_closed_enum() {
    let t: Result<TaskStatus, _> = serde_json::from_str("\"reviewing\"");
    assert!(t.is_err(), "unknown status must reject");
    let ok: TaskStatus = serde_json::from_str("\"in_progress\"").unwrap();
    assert_eq!(ok, TaskStatus::InProgress);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test plan::types -- --nocapture` → Expected: FAIL (types undefined).

- [ ] **Step 3: Implement the types** (mirror spec §3). Use `serde` with `rename_all = "snake_case"` for enums; `id` via `uuid::Uuid::new_v4()` (uuid already in tree — verify with `grep '^uuid' Cargo.toml`, else add). Timestamps are `u64` epoch-ms passed in by callers (NOT `Date::now` in pure code).

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus { Active, Archived }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus { Pending, InProgress, Blocked, Done, Cancelled }

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ElementCanvas { pub x: Option<f64>, pub y: Option<f64>, #[serde(default)] pub collapsed: bool }

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct OrchestratorRef { pub pane_id: Option<String>, pub session_id: Option<String>, pub title: Option<String> }
impl OrchestratorRef { pub fn from_pane(p: &str) -> Self { Self { pane_id: Some(p.into()), ..Default::default() } } }
pub type AgentRef = OrchestratorRef;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TransferRecord { pub from: OrchestratorRef, pub to: OrchestratorRef, pub ts: u64, pub reason: String }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MilestoneKind { pub key: String, pub label: String, pub color: Option<String> }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Section { pub id: String, pub title: String, #[serde(default)] pub body: String,
    #[serde(default)] pub subsections: Vec<Section>, #[serde(default)] pub canvas: ElementCanvas }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Milestone { pub id: String, pub from: AgentRef, pub kind: String, #[serde(default)] pub text: String,
    pub ts: u64, #[serde(default)] pub links: Vec<String>, pub loop_id: Option<String>,
    pub iteration: Option<u32>, #[serde(default)] pub canvas: ElementCanvas }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Task { pub id: String, pub name: String, #[serde(rename = "type")] pub kind: String,
    pub model: Option<String>, pub model_rationale: Option<String>, pub status: TaskStatus,
    pub status_label: Option<String>, #[serde(default)] pub section_ids: Vec<String>, #[serde(default)] pub body: String,
    pub intent: Option<String>, pub assigned_pane_id: Option<String>, pub peer: Option<String>,
    #[serde(default)] pub canvas: ElementCanvas, #[serde(default)] pub milestones: Vec<Milestone> }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoopStage { pub key: String, pub role: String, pub model: Option<String>, pub why: Option<String>, pub prompt: Option<String> }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LoopStatus { Running, Done, Exhausted, Stopped }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoopSpec { pub id: String, pub target: String, #[serde(default)] pub stages: Vec<LoopStage>,
    pub budget: u32, #[serde(default)] pub count: u32, pub stop_when: Option<String>, pub status: LoopStatus }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanvasElement { pub id: String, pub kind: String, #[serde(default)] pub canvas: ElementCanvas,
    #[serde(default)] pub content: serde_json::Value, #[serde(default)] pub links: Vec<String> }

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PlanCanvas { pub viewport: Option<Viewport> }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Viewport { pub pan_x: f64, pub pan_y: f64, pub scale: f64 }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Plan {
    pub id: String, pub title: String, #[serde(default)] pub sections: Vec<Section>,
    pub status: PlanStatus, pub owner: OrchestratorRef, pub project_path: String,
    pub created_at: u64, pub updated_at: u64,
    #[serde(default)] pub milestone_kinds: Vec<MilestoneKind>, #[serde(default)] pub loops: Vec<LoopSpec>,
    #[serde(default)] pub elements: Vec<CanvasElement>, #[serde(default)] pub tasks: Vec<Task>,
    #[serde(default)] pub canvas: PlanCanvas, #[serde(default)] pub owner_history: Vec<TransferRecord>,
}
impl Plan {
    pub fn new(title: String, project_path: String, owner: OrchestratorRef) -> Self {
        let id = format!("pl_{}", uuid::Uuid::new_v4().simple());
        Self { id, title, sections: vec![], status: PlanStatus::Active, owner, project_path,
            created_at: 0, updated_at: 0, milestone_kinds: vec![], loops: vec![], elements: vec![],
            tasks: vec![], canvas: PlanCanvas::default(), owner_history: vec![] }
    }
}
```
And `mod.rs`:
```rust
pub mod types;
pub mod coverage;
pub mod loop_engine;
pub use types::*;
```

- [ ] **Step 4: Run test to verify it passes** — `cd src-tauri && cargo test plan::types` → PASS. Then `cargo fmt && ./build.sh check`.

- [ ] **Step 5: Commit** — `git add -A && git commit -m "feat(plan): canvas plan data types"`

---

### Task 2: Coverage computation (pure)

**Files:**
- Create: `src-tauri/src/plan/coverage.rs`
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Failing test**

```rust
#[test]
fn coverage_counts_tasks_per_section() {
    let cov = compute_coverage(&plan_with(/*sections*/ &["s1","s2"],
        /*tasks*/ &[(&["s1"], TaskStatus::Done), (&["s1"], TaskStatus::Pending)]));
    assert_eq!(cov.per_section["s1"].total, 2);
    assert_eq!(cov.per_section["s1"].done, 1);
    assert_eq!(cov.per_section["s2"].total, 0);
    assert!((cov.plan_covered_fraction - 0.5).abs() < 1e-9); // 1 of 2 sections has >=1 task
}
```
(Write a small `plan_with(sections, tasks)` test helper in the same module.)

- [ ] **Step 2: Run → FAIL.** `cargo test plan::coverage`.

- [ ] **Step 3: Implement** — pure function over `&Plan`, walking the section tree (incl. subsections) and `task.section_ids`.

```rust
use std::collections::HashMap;
use super::types::{Plan, TaskStatus};

#[derive(Debug, Default, PartialEq)]
pub struct SectionCoverage { pub total: usize, pub done: usize, pub in_progress: usize }
#[derive(Debug, Default, PartialEq)]
pub struct Coverage { pub per_section: HashMap<String, SectionCoverage>, pub plan_covered_fraction: f64 }

pub fn compute_coverage(plan: &Plan) -> Coverage {
    let mut per: HashMap<String, SectionCoverage> = HashMap::new();
    fn walk<'a>(s: &'a [super::types::Section], ids: &mut Vec<&'a str>) {
        for sec in s { ids.push(&sec.id); walk(&sec.subsections, ids); }
    }
    let mut ids = vec![]; walk(&plan.sections, &mut ids);
    for id in &ids { per.entry((*id).into()).or_default(); }
    for t in &plan.tasks {
        for sid in &t.section_ids {
            let e = per.entry(sid.clone()).or_default();
            e.total += 1;
            match t.status { TaskStatus::Done => e.done += 1, TaskStatus::InProgress => e.in_progress += 1, _ => {} }
        }
    }
    let covered = ids.iter().filter(|id| per.get(**id).map_or(false, |c| c.total > 0)).count();
    let frac = if ids.is_empty() { 0.0 } else { covered as f64 / ids.len() as f64 };
    Coverage { per_section: per, plan_covered_fraction: frac }
}
```

- [ ] **Step 4: Run → PASS.** `cargo test plan::coverage && cargo fmt && ./build.sh check`.
- [ ] **Step 5: Commit** — `git commit -am "feat(plan): coverage computation"`.

---

### Task 3: Loop budget engine (pure)

**Files:**
- Create: `src-tauri/src/plan/loop_engine.rs`
- Test: inline

- [ ] **Step 1: Failing test** — budget guard + stop_when + status transitions.

```rust
#[test]
fn loop_advances_until_budget() {
    let mut l = LoopSpec { id: "lp".into(), target: "x".into(), stages: vec![], budget: 2, count: 0,
        stop_when: None, status: LoopStatus::Running };
    assert!(can_advance(&l));            // 0 < 2
    advance(&mut l); assert_eq!(l.count, 1);
    advance(&mut l); assert_eq!(l.count, 2);
    assert!(!can_advance(&l));            // 2 >= 2
    assert_eq!(l.status, LoopStatus::Exhausted);
}
#[test]
fn loop_stops_on_signal() {
    let mut l = LoopSpec { budget: 10, ..running("lp") };
    stop_for(&mut l);
    assert_eq!(l.status, LoopStatus::Done);
    assert!(!can_advance(&l));
}
```

- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** `can_advance(&LoopSpec) -> bool`, `advance(&mut LoopSpec)` (increment, set `Exhausted` when `count >= budget`), `stop_for(&mut LoopSpec)` (set `Done`). Pure; no IO.
- [ ] **Step 4: Run → PASS.** `cargo fmt && ./build.sh check`.
- [ ] **Step 5: Commit** — `git commit -am "feat(plan): loop budget engine"`.

---

### Task 4: PlanStore — atomic file persistence + index

**Files:**
- Create: `src-tauri/src/service/plan/mod.rs`
- Create: `src-tauri/src/service/plan/paths.rs`
- Modify: `src-tauri/src/service/mod.rs` (add `pub mod plan;`)
- Test: `src-tauri/src/service/plan/tests.rs` (declared `#[cfg(test)] mod tests;`)

- [ ] **Step 1: Failing test** — save → file exists; load → equal; index lists it; atomic (no partial file).

```rust
#[test]
fn store_saves_loads_indexes() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new("T".into(), dir.path().display().to_string(), OrchestratorRef::from_pane("abc"));
    store.save(&mut p, 1000).unwrap();                       // 1000 = injected now-ms
    let path = dir.path().join(".codeterm/plans").join(format!("{}.json", p.id));
    assert!(path.exists());
    let loaded = store.get(&p.id).unwrap();
    assert_eq!(loaded.title, "T");
    assert!(store.list_index().iter().any(|e| e.plan_id == p.id));
    assert_eq!(loaded.updated_at, 1000);
}
```
(`tempfile` is a dev-dep; verify in `Cargo.toml [dev-dependencies]`, add if missing.)

- [ ] **Step 2: Run → FAIL.** `cargo test service::plan`.
- [ ] **Step 3: Implement `PlanStore`:**
  - In-memory: `RwLock<HashMap<String, Arc<Mutex<Plan>>>>` keyed by plan_id + an index `Vec<IndexEntry>`.
  - `save(&self, plan: &mut Plan, now_ms: u64)`: set `plan.updated_at = now_ms` (and `created_at` if 0), serialize pretty, write to `<dir>/<id>.json.tmp`, `fs::rename` → atomic. Update map + index. (`now_ms` is injected — never call a clock in the store; the route layer passes `epoch_ms()`.)
  - `get(&self, id) -> Option<Plan>`, `list_index(&self) -> Vec<IndexEntry>`, `delete(&self, id)`.
  - `paths.rs`: `plans_dir(project_path) -> PathBuf` = `<project>/.codeterm/plans`, created on demand; `project_root(cwd)` = `git rev-parse --show-toplevel` result if available else `cwd` (use the repo's existing git-toplevel helper if one exists — `grep -r "show-toplevel" src-tauri/src`; else shell out once at resolve time, cached).
  - `new_for_test(dir)` constructs a store rooted at a temp dir.
- [ ] **Step 4: Run → PASS.** `cargo fmt && ./build.sh check`. Keep `mod.rs` < 1000 lines; if over, split store vs index.
- [ ] **Step 5: Commit** — `git commit -am "feat(plan): PlanStore atomic persistence + index"`.

---

### Task 5: PlanStore mutations — sections, tasks, milestones, archive, transfer

**Files:**
- Modify: `src-tauri/src/service/plan/mod.rs` (add mutation methods)
- Test: `src-tauri/src/service/plan/tests.rs`

- [ ] **Step 1: Failing tests** — one per mutation; assert state + that `updated_at` advances. Cover: `add_section`, `add_task` (many-to-many section_ids), `close_task` (status=Done + optional result milestone), `add_milestone` (rejects undeclared kind unless `add_kind`), `archive`/`unarchive`, `transfer`/`disown`/`claim` (owner + owner_history), `set_layout`.

```rust
#[test]
fn add_milestone_rejects_undeclared_kind() {
    let (store, id, tid) = store_with_task(&["status_update"]);
    let err = store.add_milestone(&id, &tid, "deploy", "x", AgentRef::from_pane("w"), false, 1).unwrap_err();
    assert!(matches!(err, PlanError::UndeclaredKind { .. }));
    store.add_milestone(&id, &tid, "deploy", "x", AgentRef::from_pane("w"), true, 2).unwrap(); // --add-kind
    assert!(store.get(&id).unwrap().milestone_kinds.iter().any(|k| k.key == "deploy"));
}
#[test]
fn transfer_updates_owner_and_history() {
    let (store, id, _) = store_with_task(&["s"]);
    store.transfer(&id, OrchestratorRef::from_pane("new"), "handoff", 5).unwrap();
    let p = store.get(&id).unwrap();
    assert_eq!(p.owner.pane_id.as_deref(), Some("new"));
    assert_eq!(p.owner_history.len(), 1);
}
```

- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** the methods on `PlanStore`, each locking the plan's `Mutex`, mutating, bumping `updated_at`, persisting via the Task-4 atomic save, returning a typed `Result<_, PlanError>`. Define `PlanError` enum (`NotFound`, `TaskNotFound`, `UndeclaredKind{declared:Vec<String>}`, `BudgetExhausted{count,budget}`, `BadStatus`, …) — these power the teaching errors (Task 9). Milestone `kind` validated against `milestone_kinds` unless `add_kind`. Coverage/loop helpers from Tasks 2–3 reused.
- [ ] **Step 4: Run → PASS.** Watch the 1000-line budget; split into `mutations.rs` if needed. `cargo fmt && ./build.sh check`.
- [ ] **Step 5: Commit** — `git commit -am "feat(plan): store mutations (sections/tasks/milestones/transfer)"`.

---

### Task 6: File-watcher reload

**Files:**
- Create: `src-tauri/src/service/plan/watcher.rs`
- Modify: `src-tauri/src/service/plan/mod.rs` (spawn watcher; `reload_from_disk(id)`)
- Test: `tests.rs`

- [ ] **Step 1: Failing test** — write a plan, mutate the JSON file externally, call `reload_from_disk`, assert the in-memory copy reflects the edit and the index `updated_at` refreshed.
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** `reload_from_disk(&self, id)` (re-read + replace map entry + reindex) as the pure, testable core. Then `watcher.rs` uses the `notify` crate (verify/add to Cargo.toml) to watch `.codeterm/plans/` of open projects and call `reload_from_disk` on external change, emitting `plan-changed` (Task 7 event helper). Debounce 200ms; ignore events whose mtime matches our own last atomic write (avoid self-trigger loops).
- [ ] **Step 4: Run → PASS.** `cargo fmt && ./build.sh check`.
- [ ] **Step 5: Commit** — `git commit -am "feat(plan): external-edit file watcher"`.

---

### Task 7: REST routes + `plan-changed` event

**Files:**
- Create: `src-tauri/src/api/plan_routes.rs`
- Modify: `src-tauri/src/api/routes.rs` (register routes)
- Test: a route-level test mirroring existing `api` tests, or exercise via the eval in Task 12.

- [ ] **Step 1: Failing test** — POST create returns 200 + plan_id; GET list includes it. Follow the existing axum handler test pattern in `src-tauri/src/api/` (`grep -rl "AxumState<AppState>" src-tauri/src/api | head`).
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** handlers for the §4 REST set (create/list/get/update/archive/delete/transfer/disown/claim; task add/update/close/assign/rm; milestone add/update/rm; loop create/set/status/stop; section add/update/rm; note/element add; layout set). Each: parse body → call `PlanStore` (passing `epoch_ms()` computed HERE, at the IO edge) → on success emit `plan-changed`. **Verified:** use the existing `crate::api::events::emit_everywhere(&state.svc.app_handle, "plan-changed", json!({"plan_id": id}))` — it already fans out to BOTH the Tauri event bus AND the SSE/websocket feed (`src-tauri/src/api/events.rs:315`). One call, both transports; no new helper. Register routes in `routes.rs` next to `/api/panes/...` (pattern at `routes.rs:102`).
- [ ] **Step 4: Run → PASS.** `cargo fmt && ./build.sh check`. Keep `plan_routes.rs` < 1000 lines; split by noun (`plan_routes/tasks.rs`, etc.) if needed.
- [ ] **Step 5: Commit** — `git commit -am "feat(plan): REST routes + plan-changed event"`.

---

### Task 8: CLI `codeterm plan ...`

**Files:**
- Create: `cli/src/args/plan.rs`, `cli/src/cmd/plan.rs`
- Modify: `cli/src/args/cli.rs` (add `Plan { #[command(subcommand)] action: PlanAction }`), `cli/src/main.rs` (dispatch), `cli/src/cmd/manifest.rs` (register so `prompt_cli_commands_exist` passes)
- Test: the `evals/plan-lifecycle` bench (Task 12) is the integration oracle.

- [ ] **Step 1: Write the arg tree** `PlanAction` mirroring spec §5. ALL free-text args carry `#[arg(allow_hyphen_values = true)]` and a `--<field>-file` sibling + stdin fallback (copy the exact pattern from the mem-write fix in `cli/src/args/mem.rs`). `--json` on reads.
- [ ] **Step 2: Implement handlers** in `cmd/plan.rs` — each builds a JSON body and calls the socket client method `plan.<verb>` (same shape as `cmd/agent.rs` `agent.report`). Reads pretty-print markdown by default, raw JSON with `--json`.
- [ ] **Step 3: Wire the socket dispatch (VERIFIED — this is the real bridge, not HTTP).** The CLI talks JSON-RPC over a Unix socket (`cli/src/client.rs` → `client.call("plan.create", params)`); the daemon dispatches by method prefix (`src-tauri/src/socket/server/mod.rs:~373` `match prefix { "agent" => agent::dispatch(...), ... }`). So:
  - **Create** `src-tauri/src/socket/server/plan.rs` with `pub(super) fn dispatch(method, params, app, svc) -> Result<Value, JsonRpcError>` — copy `src-tauri/src/socket/server/agent.rs` as the template; `match method { "plan.create" => ..., "plan.get" => ..., ... }`, each extracting params and calling `svc.<plan_store_method>`, returning `serde_json::json!({...})`.
  - **Register** `"plan" => plan::dispatch(method, &params, app, svc),` in the `mod.rs` prefix match (next to `"agent"`), and `mod plan;` at the top.
  - **Capabilities:** add the `plan.*` method names to the `system.capabilities` response in `socket/server/mod.rs` so `codeterm capabilities` lists them.
- [ ] **Step 4: Keep the manifest test green (VERIFIED gap).** `cli/src/cmd/manifest.rs`'s `prompt_cli_commands_exist` test walks `src-tauri/prompts/codeterm_cli.md` and fails if a documented `codeterm <cmd>` isn't in the clap tree — AND vice-versa it's the gate that the prompt lists real commands. Add the minimal `codeterm plan ...` command lines to `codeterm_cli.md` now (the full "Plans & Canvas" prose lands in Phase 4) so the test passes. File-arg naming: use `--<field>-file` consistently and `-` as the path to mean stdin (match `cli/src/cmd/agent.rs::resolve_spawn_task`). Run `cd src-tauri && cargo test prompt_cli_commands_exist`.
- [ ] **Step 5: Build + smoke** — `./build.sh cli` then against dev (port 7686, dev token): `codeterm plan create --title T` → prints an id; `codeterm plan get <id>` → renders; `codeterm capabilities | grep plan`. (Dev env: `export CODETERM_API_PORT=7686; export CODETERM_API_TOKEN=$(grep -E '^token = ' ~/.codeterm-dev/config.toml | head -1 | sed 's/.*"\(.*\)"/\1/')`.) Expected: clean create/get round-trip.
- [ ] **Step 6: Commit** — `git commit -am "feat(cli): codeterm plan handles + socket dispatch"`.

---

### Task 9: Teaching errors

**Files:**
- Modify: `cli/src/cmd/plan.rs` (render `PlanError` → actionable message)
- Modify: `src-tauri/src/api/plan_routes.rs` (return structured error payloads carrying the fix hint)
- Test: extend `evals/plan-lifecycle` to assert the messages.

- [ ] **Step 1: Failing eval assertions** — bad kind / bad status / unknown id / dash-leading body / loop-exhausted each print `✗` + a runnable fix line (spec §4.3).
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** — map each `PlanError` variant to the exact teaching string from §4.3 (include the declared-kinds list, the valid status enum, `plan list --active`, the `--field=`/`--field-file` hint, the `plan loop set --budget` hint). The CLI prints them to stdout and exits non-zero.
- [ ] **Step 4: Run → PASS.**
- [ ] **Step 5: Commit** — `git commit -am "feat(plan): self-correcting teaching errors"`.

---

### Task 10: Report → milestone linkage + plan-owner routing + env binding

**Files:**
- Modify: `src-tauri/src/api/agent_routes.rs` (`ReportBody` gains `plan_id`, `task_id`)
- Modify: `src-tauri/src/service/hierarchy.rs` (`submit_report`: if `plan_id` set → append milestone; resolve report target via plan→current-owner lookup)
- Modify: `src-tauri/src/service/pty_io/lifecycle.rs` (export `CODETERM_PLAN_ID`/`CODETERM_TASK_ID` when a Tab is task-bound)
- Modify: `cli/src/args/agent.rs` + `cli/src/cmd/agent.rs` (`--plan`, `--task` on `agent report`)
- Test: service test + eval

- [ ] **Step 1: Failing test** — `submit_report` with `plan_id`/`task_id` appends a `Milestone` to that task (kind defaults to `status_update`), `from` auto-resolved from the caller Tab; and a transferred plan routes the report to the NEW owner.
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement (VERIFIED anchors).** Extend `ReportBody` (`agent_routes.rs:191`) with `plan_id: Option<String>, task_id: Option<String>`. In `submit_report` (`hierarchy.rs:45`), when `plan_id` is set, call `PlanStore::add_milestone` with `from` resolved from the reporting Tab; map report status (done/partial/blocked) to a milestone text prefix. Override `resolve_report_target` (`hierarchy.rs:225`, the verified `parent → report_target → spawned_by → system_inbox` chain) with `PlanStore::owner_of(plan_id)` when `plan_id` is present. Export env in `pane_env` (`pty_io/lifecycle.rs:11–54`, right after the `worker_id` block) — only when the Tab is task-bound. Reports WITHOUT `plan_id` keep today's exact behavior (regression-guard test).
- [ ] **Step 4: Run → PASS.** `cargo fmt && ./build.sh check`.
- [ ] **Step 5: Commit** — `git commit -am "feat(plan): report→milestone linkage + owner routing + env binding"`.

---

### Task 11: Plan-adherence reminders

**VERIFIED GAP:** there is NO per-tab turn counter in the codebase today (`agent_tracker` tracks status, not turns). This task **creates** the counter, not just the decision logic.

**Files:**
- Create: `src-tauri/src/service/plan/reminders.rs` (pure decision + the counter state struct)
- Modify: `src-tauri/src/service/hierarchy.rs` — increment the counter where agent activity is observed; reset on plan-touch
- Modify: `src-tauri/src/service/plan/mod.rs` — every `PlanStore` mutation + `submit_report --plan` resets that Tab's counter
- Modify: `src-tauri/src/config/schema/mod.rs` (or wherever the config schema structs live — verify with `grep -rn "pub struct .*Config" src-tauri/src/config`) — add `OrchestrationConfig { plan_reminders: bool, nudge_after_turns: u32 }` with serde defaults `true`/`12`, wired into the root config like the existing `memory`/`mesh` sections
- Modify: reuse `crate::service::messenger::Messenger::deliver` with `MessageKind::Reminder` (VERIFIED reusable — `messenger.rs:198`, already supports the Reminder kind) to inject the nudge
- Test: `reminders.rs` inline

- [ ] **Step 1: Failing test** — pure `should_nudge(turns, last_nudge, cfg) -> Option<NudgeKind>`: returns None below threshold and when `plan_reminders=false`; fires at `nudge_after_turns`; interval grows ×1.5 after each nudge; selects Worker vs Orchestrator copy by role. Plus a `TurnCounter` test: increment advances, `reset` zeroes.
- [ ] **Step 2: Run → FAIL.** `cargo test plan::reminders`.
- [ ] **Step 3: Implement** — a `TurnCounter` state (`Mutex<HashMap<String, (u32 turns, u32 last_nudge)>>` on `ServiceContext`); increment it in `submit_report` (after the report is stored, `hierarchy.rs:~63`) and reset it from every `PlanStore` mutation + from `submit_report` when `--plan` is present. The pure `should_nudge` decides; on a hit, build the role-specific copy (spec §4.2 drafts) and inject via `Messenger::deliver(..., MessageKind::Reminder, ...)`. Add `OrchestrationConfig` to the config schema with defaults.
- [ ] **Step 4: Run → PASS.** `cargo fmt && ./build.sh check`.
- [ ] **Step 5: Commit** — `git commit -am "feat(plan): plan-adherence reminders + turn counter"`.

---

### Task 12: `evals/plan-lifecycle` bench

**Files:**
- Create: `evals/plan-lifecycle/README.md`, `evals/plan-lifecycle/run.sh`

- [ ] **Step 1:** Write `run.sh` that sources `evals/_lib/lib.sh` (so `dc` is dev-locked) and drives the full lifecycle through `dc plan ...`: create → section add (+ `--spec-file` auto-split) → task add (many-to-many `--sections`) → assign → `dc agent report --plan --task` → assert milestone appended (`dc plan get --json | jq`) → close → loop create + exhaust → archive → `plan list --unowned` after owner death. Assert each teaching error fires on bad input. Model-judge optional; primary oracle = `jq` assertions on `plan get --json`.
- [ ] **Step 2: Run** against dev; capture baseline. Expected: all assertions pass.
- [ ] **Step 3: Commit** — `git commit -am "test(plan): plan-lifecycle eval"`.

**Phase 1 exit gate:** `./build.sh check` green, all `cargo test plan` + `service::plan` green, `evals/plan-lifecycle` green against dev, file-size check passes. A user can now run the full `codeterm plan` flow with zero UI.

---

## PHASE 2 — Read-only Canvas (task map — expand into a plan at phase start)

Dependency: Phase 1 shipped. Frontend-only (no Rust build).

| Task | Files | Responsibility | Gate |
|------|-------|----------------|------|
| 2.1 Viewport hook | `Canvas/useCanvasViewport.ts` | Extract Mesh pan/zoom (middle-drag, wheel, fit-to-view, transform). | Unit (transform math); Mesh still works after retrofit. |
| 2.2 Mesh retrofit | `MeshPanel/MeshCanvas.tsx` | Replace inline transform with the hook. | Playwright mesh smoke unchanged. |
| 2.3 Data hook | `Canvas/usePlans.ts` | REST fetch + subscribe `plan-changed` via `listen()` from `src/lib/bridge.ts` (VERIFIED: transparently routes Tauri-event native / SSE `/api/events/stream` web — no branching). | Mock-fetch unit; realtime test (emit → re-render). |
| 2.4 Settings | `Canvas/settings.ts` | `CanvasLayoutSettings` typed defaults (§6.2). NO magic numbers downstream. | Type test. |
| 2.5 Layout engine | `Canvas/layout/*` | Two-phase deterministic layout: intra-plan frame (sections portyanka → tasks → milestone lanes, chains width-bounded + wrap/cluster, loops folded) → region packing (bin-pack, AABB no-overlap). Reads only settings. | Determinism + no-overlap unit (`opus`). |
| 2.6 Focus detection | `Canvas/layout/focus.ts` | viewport center → focused plan + in-view docs. | Unit: center → correct plan/docs. |
| 2.7 Cards | `Canvas/cards/*` | `SectionCard`/`TaskCard`(marker+name+model chip+`why`+status badge+coverage)/`MilestoneNode`/`PlanFrame`; markdown via `RichMarkdown` from `@codeterm/chat-ui` (VERIFIED export); LOD by zoom (name-only < 0.5, full ≥ 0.5); virtualized mount via `getVisibleNodeIds(cards, viewport, size)` from `MemoryPanel/lib/viewport.ts` (VERIFIED reusable). | Playwright web-mode render. |
| 2.8 CanvasView | `Canvas/CanvasView.tsx` | `mode="workspace"`/`"focus"`; compose hook+layout+cards; edges as plain lines. | Playwright. |
| 2.9 Workspace mount | `App.tsx` (~641–646) | Replace the current "Loading..." div in the `mainView === "workspace"` empty-state branch with `<CanvasView mode="workspace">` (VERIFIED: that div at 641–646 is the exact empty-state slot). | Playwright: select workspace → canvas shows. |
| 2.10 Plan button | `Terminal/PaneTopBar.tsx` (~52–54) | `<PlanButton>` (orchestrator + worker Tabs), opens focus view, **pulses** on `plan-changed`. | Playwright: button shows + pulses + opens. |

Models: `cursor:composer-2.5` for cards/CSS (2.7) and mounts (2.9/2.10); `opus`/`sonnet` for hook (2.1), layout engine (2.5), focus (2.6). Each lane = own worktree; orchestrator integrates into warm checkout, runs `npm run typecheck` + Playwright web-mode.

## PHASE 3 — Interactivity (task map)

Dependency: Phase 2. `CanvasComposer.tsx` (floating, context-addressed → owning orchestrator via existing send path, target chip, auto-attach in-view docs); manual drag → `plan layout set` persistence; spawn-orchestrator / assign-task from empty canvas (calls `workspace create` + `agent spawn` + `plan task assign`, incl. `--mesh`); archive UX + unowned tray + claim; **Canvas debug panel** (live-tune `CanvasLayoutSettings`, save to `config.toml [canvas]`). Models: `opus` for composer routing + focus wiring; `composer-2.5` for panel/drag UI.

## PHASE 4 — Prompts/docs (task map)

Dependency: CLI stable (Phase 1). Create `src-tauri/prompts/orchestration_toolset.md` (the `codeterm docs ORCHESTRATION` cascade — mechanics only); add the "Plans & Canvas" pointer to `codeterm_cli.md`; append plan paragraphs to `orchestrator_suffix.md` / `worker_suffix.md` (spec §7 drafts); write `docs/CANVAS.md`; add a one-line toolset pointer to the `codeterm-orchestrator` mem skill (principle stays). Bump every touched doc's `<!-- revision -->`. Update `docs/API.md`, `docs/PROMPTS.md`, `docs/CONFIG.md`. Models: `sonnet` (docs/prompts), `opus` review.

---

## Build allocation & parallelization (Phase 1)

Phase 1 has a serial spine (types → store → mutations) then a fan-out:
- **Serial first (one strong worker, `opus`/`sonnet`):** Tasks 1→2→3→4→5 (types + pure logic + store) — everything else depends on these.
- **Then parallel lanes** off the store:
  - Lane A (`opus`/`sonnet`): Task 6 watcher, Task 10 report linkage/routing (touches hierarchy — careful, review).
  - Lane B (`composer-2.5`/`codex`): Task 7 REST routes, Task 8 CLI (mechanical wiring against patterns).
  - Lane C (`sonnet`): Task 9 teaching errors, Task 11 reminders, Task 12 eval.
- **Integration:** orchestrator merges lanes into the warm checkout, runs `cargo fmt && ./build.sh check` + `cargo test plan service::plan` + the eval. Worktree per lane; keep until integrated (orchestrator-skill hygiene).
- **Review:** `opus` adversarial pass on Task 5 (store concurrency/atomicity) and Task 10 (report routing / regression) before merge.

---

## Self-review (against spec)

- **Spec coverage:** §3 model → T1; coverage → T2; loops → T3/§4.4; storage+index+atomic+watcher → T4/T6; mutations incl. transfer/claim/disown → T5; teaching errors → T9; report linkage + env + owner routing → T10; reminders → T11; REST+event → T7; CLI → T8; eval → T12. Frontend §6 → Phase 2 map; composer/interactivity §6/§4.x → Phase 3; prompts §7 → Phase 4; mesh task ops §4.7 → assign/close `--mesh` in T8 + routing in T10. Deferred items (§9) intentionally unbuilt.
- **Placeholder scan:** Phase 1 steps carry real code/commands; Phases 2–4 are explicitly task-maps to be expanded (not placeholders inside an executable task).
- **Type consistency:** `PlanStore::{save,get,list_index,add_section,add_task,close_task,add_milestone,archive,transfer,disown,claim,owner_of,reload_from_disk,set_layout}`, `PlanError` variants, and the `types.rs` structs are referenced consistently across T1–T11; `now_ms` injected into the store, `epoch_ms()` computed only at the route/IO edge.

## Verification pass (2026-06-23, 5 read-only agents vs live code)

Confirmed against real code: `uuid`/`tempfile`/`notify`(+debouncer) present; module-decl points (`lib.rs`, `service/mod.rs`); `submit_report`/`ReportBody`/`resolve_report_target`/`pane_env` exact anchors; `emit_everywhere` dual-emit helper (`api/events.rs:315`); axum + route patterns; CLI `Commands`/`main.rs`/`manifest.rs`/mem-write `allow_hyphen_values` pattern; JSON-RPC-over-socket transport; frontend `App.tsx` "Loading..." empty-state slot, `PaneTopBar` insertion, `MeshCanvas` extractable transform, `RichMarkdown`@chat-ui, `getVisibleNodeIds`, `listen()` bridge (native + SSE web).

Gaps found and patched into this plan:
1. **Socket dispatch submodule** (`socket/server/plan.rs` + `"plan"` prefix + capabilities) — was vague in T8, now explicit (CLI is JSON-RPC-over-socket, not HTTP).
2. **Turn counter does not exist** — T11 now *creates* it, not just the decision logic.
3. **`[orchestration]` config section + `prompt_cli_commands_exist` manifest gate** — added to T8/T11 (minimal `codeterm plan` lines into `codeterm_cli.md` to keep the test green).
4. **`git project_root` helper is new** — already created in T4 `paths.rs` (fallback to cwd).
```
