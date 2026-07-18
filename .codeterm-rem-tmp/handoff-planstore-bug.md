# HANDOFF to Orchestrator (Dumas) — plan store is non-functional; take into work

Owner (Andrey) directive: plans are not working right now — Dumas takes this problem into work.

## P0 BUG — plan writes re-index the ENTIRE plan on every mutation

**Symptom:** every `plan section add` / `plan task add` makes the daemon DELETE + re-embed
EVERY section and EVERY task of the target plan into mem. Measured in prod log: **373
`mem.index` ops per single write window**, ~34 distinct plan-units re-indexed per write.
Cost is O(N) per write where N = whole-plan size; batch-adding grows N → O(N²).

**Consequence on the fat plan (pl_as7ty5, ~66 units):** plan writes NO LONGER COMPLETE.
A single `plan task add` hung >4 min and never committed to disk (task_yzmbbw:
"creating" 10:15:57 → "FAILED to land" 10:20:02, absent on disk after). Basic RPC is
fine — `identify` is instant. It is plan-write-specific and plan-size-driven.

**Root cause class:** relative of GitHub #236 (whole-plan read/modify/write not
incremental). Fix direction: index only the changed unit on a mutation, not the whole
plan; and/or move the mem re-embed off the write's critical path (spawn_blocking /
background), per AGENTS.md async-hot-path discipline.

## P0 BUG (separate) — plan-storage migration dropped an active plan

The 2026-07-18 plan-storage layout migration (added `sections/ handoffs/ attachments/`
subdirs) lost active plan **pl_ghk358** entirely — no backup in active/archive/user data.
Migration must never drop an active plan; needs a pre-migration backup + a
migrate-or-fail invariant.

## Recovery state (data is SAFE)

- Full verbatim export: `.codeterm-rem-tmp/pl_ghk358-recovery.md`
  **sha256 78a67aaf79b07838e39251ebaff501a63bc3321b6f1ebc8cc1085a7a85ef2e07**
  (goal + 9 spec sections + 14 phases + all 39 task briefs/criteria/test-plans + 7 strides).
- Restored into pl_as7ty5 so far: 9 spec sections + 14 phases (under parents
  "PTY delivery guarantee (recovered from pl_ghk358)" spec sec_46l3he / plan sec_47k6nr).
- **0 of 30 todo/review tasks landed** — every task write timed out on the degraded daemon.
- Restorer script (idempotent, disk-id harvest, correct DONE/CANCELLED skip):
  `.codeterm-rem-tmp/restore_tasks.py`; progress log `.codeterm-rem-tmp/restore-progress.log`.
  Re-runnable once the store is healthy — it skips already-created tasks by name.

## Decision needed (Orchestrator/owner call — touches prod 7685)

1. Restart the prod daemon (unwedge), then re-run restore into pl_as7ty5 — but the
   reindex bug remains, so 30 adds to a 66-unit plan is still ~20-40 min (O(N²)).
2. Put the 30 recovered tasks in a FRESH lean plan — reindex O(N) on tiny N, lands in
   minutes without restart; deviates from "everything in pl_as7ty5".
3. Fix the incremental-index bug first, then restore fast into pl_as7ty5.

Planner (Curie) is ready to execute whichever path is chosen; recommends fixing the
incremental-index bug (option 3) as it also unblocks all future planning on big plans,
with option 2 as the immediate-restore stopgap.
