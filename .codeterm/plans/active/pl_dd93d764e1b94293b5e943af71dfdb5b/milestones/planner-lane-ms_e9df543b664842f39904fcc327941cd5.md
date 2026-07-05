---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_e9df543b664842f39904fcc327941cd5
handle: planner-lane-ms_e9df543b664842f39904fcc327941cd5
title: status_update
---

PARTIAL: Canonical sidecar architecture planning complete (incl. remote workers).
SPEC: new section canonical-sidecar — plan sidecars get exactly one home: daemon-owned state + canonical_project_path per plan (default main-worktree root at creation, --project override); ALL .codeterm/plans|specs writes route there regardless of caller cwd/worktree; lane diffs stay clean; doctor detects stray copies, no shims. Remote extension per Andrey's follow-up: one OWNING DAEMON per plan; remote mesh workers' mutations auto-home to the owning peer (lane env + mesh-origin metadata; explicit --mesh wins); no foreign-plan sidecars on remote machines; offline owner fails loudly naming the peer; no v1 offline queue.
PLAN: new wave p1-canonical-sidecar with traced root cause (service/plan/paths.rs:15-18 linked-worktree-owns-store rule + committed .codeterm gives every lane a store copy) and full writer inventory (mod.rs, body_files, spec_files, watcher, search_index). Spec-linked; coverage 100% (15/15).
TASKS: p1-sidecar-routing (task_bf6e1826, p1, opus — local routing, model field, doctor stray check, live worktree smoke) and p1b-mesh-homing (task_ea183935, p1, opus — owning-peer identity, auto-homing decision, two-peer smoke or explicit accepted gap; depends on p1-sidecar-routing, same lane sequential). Discovery folded into task step 1 — no separate discovery wave.
DEPENDENCY DECISION (recorded in wave body): P1 blocks UX lanes t1-t7 together with P0 — every UX lane worker's reports/strides write sidecars and would re-pollute lane worktrees; P1 runs PARALLEL to the active p0-report-reliability lane (disjoint areas: service/plan storage vs cli report path). t0/t0b/t0c may also run parallel.
NEXT: dispatch p1-sidecar-routing now to a second rust worker in its own worktree; p1b follows in the same lane; hold t1-t7 until BOTH P0 and P1 live smokes pass.
