---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_bf6e1826621840d4a8e7298ccfd757de
handle: p1-sidecar-routing
title: 'P1: canonical plan sidecar ownership (local routing)'
status: done
---

Implement canonical plan sidecar ownership (local half). BLOCKS UX lane dispatch (t1-t7) together with p0-report-reliability; runs parallel to it in its own lane (disjoint areas: service/plan storage vs cli report path).

Root cause (traced — verify, don't re-discover): paths.rs::project_root(cwd) resolves per-caller-cwd; a linked worktree carrying its own .codeterm/ claims store ownership (src-tauri/src/service/plan/paths.rs:15-18). Repos that commit .codeterm/plans give every lane worktree a store copy at branch time, so worker plan mutations (including strides from reports) materialize sidecars in lane worktrees. Split-brain.

Steps:
1. Trace inventory first (cheap, in-task): all plans_dir()/specs_dir()/project_root() callers — service/plan/mod.rs, body_files.rs, spec_files.rs, watcher.rs, search_index — confirm every sidecar write path.
2. Plan model: add canonical_project_path (serde + persistence round-trip). plan create sets it: default = main-worktree root resolved from caller cwd; --project flag override. Expose in plan get/plan state; allow explicit change via plan update --project (records a stride).
3. Rewrite paths.rs resolution: plan-id -> canonical path from daemon state for ALL writes; delete the linked-worktree-owns-store rule (no shims); cwd-based resolution survives only for creation default + project-scoped discovery (plan list).
4. Route every writer (body_files, spec_files, watcher watch-roots, search_index, mod.rs callers) through the canonical path; no writer derives targets from caller cwd.
5. Doctor storage check: stray .codeterm/plans/<plan_id> copies in non-canonical locations flagged with canonical path + cleanup hint; no auto-merge.
6. Tests: creation default; mutation issued from a linked-worktree cwd writes only canonical; two plans with different canonical paths stay independent; doctor stray detection.
7. Live dogfood smoke (approval gate): create a scratch worktree, run plan/spec mutations + a report from it; `git status` there shows zero .codeterm changes; canonical checkout carries all of them.

Commands: ./build.sh check; narrowed cargo test (service::plan::), single runs; ./build.sh cli; the live smoke against dev 7686.
Report: inventory results, model/API changes, test output, live smoke transcript (both git statuses). Commit lane before in_review; no report id printed => codeterm send fallback.
