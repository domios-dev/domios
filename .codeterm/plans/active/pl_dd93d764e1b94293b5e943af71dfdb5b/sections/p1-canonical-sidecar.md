---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_a5ea65d174004849a94def59b60b39ac
handle: p1-canonical-sidecar
title: Wave P1 — Canonical sidecar path routing
---

Wave P1 (rust lane, parallel with Wave P0 report-reliability — different areas: service/plan storage vs cli report path; BLOCKS dispatch of all UX lanes t1-t7 together with P0, because every lane worker's reports/status updates write sidecars and would re-pollute lane worktrees):

Canonical plan sidecar ownership — path resolution + write routing + detection.

Implementation areas (inventoried):
- src-tauri/src/service/plan/paths.rs: project_root(cwd) loses the "linked worktree with own .codeterm owns the store" rule (paths.rs:15-18); introduce canonical resolution: plan-id -> canonical_project_path from daemon state; cwd-based resolution remains ONLY for (a) plan creation default (resolve to main worktree root) and (b) project-scoped discovery/list.
- Plan model (src-tauri/src/service/plan/mod.rs): add canonical_project_path to the plan record with serde/persistence; plan create sets it (default main-worktree root of caller cwd; --project flag override); CLI: show in plan get/state, allow update via plan update --project (explicit, logged as a stride).
- Sidecar writers: route every write through the canonical path — body_files.rs, spec_files.rs, watcher.rs (watch canonical dir per plan, not caller dirs), search_index, and any plans_dir()/specs_dir() caller in service/plan/mod.rs. No writer may derive its target from caller cwd.
- Doctor: new storage check — stray .codeterm/plans/<plan_id> copies in non-canonical worktrees are flagged with the canonical path and a cleanup hint (no auto-merge, no shims).
- Interplay with Wave P0: report_guard's tool-owned-path dirt exclusion remains as defense in depth, but after this wave lane worktrees should simply have no sidecar writes to exclude.

Verification: unit tests in service/plan (creation default, routing from a linked-worktree cwd, per-plan independence for two plans with different canonical paths); doctor stray detection test; ./build.sh check; single narrowed cargo test runs. Live dogfood smoke as the approval gate: worktree worker mutates plan -> worker worktree git-clean of .codeterm, canonical checkout carries the diff.

Remote-worker extension (mesh homing):
- Plan record gains owning-peer identity alongside canonical_project_path; `plan get/state` display it.
- CLI plan/spec/report commands: when the target plan is not owned by the local daemon (lane env CODETERM_PLAN_ID + lane mesh-origin metadata, or a plan-id lookup miss with a known remote owner), auto-route the mutation to the owning peer over the existing --mesh transport; explicit --mesh still wins.
- Remote mutation path writes sidecars ONLY on the owning machine's canonical checkout; the remote worker's machine gets no .codeterm/plans writes.
- Offline owner => loud structured failure naming the owning peer (aligns with REPORT NOT SENT framing); no v1 offline queue.
- Tests: mesh-routing unit tests (auto-homing decision), plus a two-peer live smoke if a second dev peer is available; otherwise the routing decision layer is unit-gated and the live two-peer smoke is recorded as an accepted gap for the mesh-e2e backlog.
