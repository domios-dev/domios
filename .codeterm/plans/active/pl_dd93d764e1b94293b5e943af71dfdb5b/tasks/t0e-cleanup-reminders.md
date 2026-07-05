---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_97337ec04aca4512be2133fbf98541e1
handle: t0e-cleanup-reminders
title: 'Cleanup-debt reminders: nudges, close, doctor, archive'
status: done
---

Surface lane-cleanup debt where orchestrators actually look: heartbeat nudges, task close output, doctor, and plan archive (spec section cleanup-hygiene; wave w0e-cleanup has the area map).

Steps:
1. Shared debt fn in service/plan: tasks done/cancelled with lane.cleanup_status != done OR assigned live pane OR recorded worktree path existing on disk. Deferral (explicit deferred cleanup_status / hint) silences the reminder but stays visible as info.
2. heartbeat.rs reconciliation_prompt: when debt non-empty, replace the generic "clean finished lane worktrees" line with the concrete list — handle, pane, worktree, branch + exact `codeterm plan task update <plan> <task> --cleanup-status done` command per lane. Keep the generic line when debt is empty.
3. Task close / done|cancelled transition: print the lane's cleanup line in CLI output; include it in the owner's next event nudge.
4. Doctor: per-lane WARNING (never error) with the fix command; deferred lanes listed as info.
5. plan archive: print the debt list prominently before archiving (non-blocking).
6. Tests: debt fn (each source + deferral), envelope with/without debt, close-time output, doctor warning, archive warning.

Commands: ./build.sh check; narrowed cargo test (service::plan), single runs; ./build.sh cli; live scratch-task smoke on dev 7686 (close with dirty lane -> observe reminder chain -> clean -> observe silence).
Report: debt definition implemented, sample nudge text, test output, live smoke transcript.
