---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_bd440bc72f134062a69d31b3cbcfe657
handle: w0e-cleanup
title: Wave 0e — Cleanup-debt reminders (nudges/doctor/archive)
---

Wave 0e (rust lane, parallel with other wave-0 work; no UX dependency): lane-cleanup debt surfaced in nudges, task close, doctor, and archive.

Implementation areas:
- Debt computation: one shared fn over plan state (tasks done/cancelled x lane.cleanup_status/pane/worktree) in src-tauri/src/service/plan (near lane_metadata.rs); worktree-exists check is a cheap fs probe, daemon-side.
- Heartbeat envelope: src-tauri/src/service/plan/heartbeat.rs reconciliation_prompt swaps the generic cleanup line (heartbeat.rs:128 area) for the concrete per-lane debt list when non-empty.
- Close-time reminder: task close/status-transition path in service/plan mutations prints lane cleanup info in CLI output and feeds the owner's event nudge.
- Doctor: cleanup/lanes warnings with exact fix commands (reuse quality doctor plumbing; warning severity, never error).
- Archive gate: plan archive prints the debt list prominently before archiving.
- Tests: debt fn unit tests (each debt source, deferral silencing), envelope rendering with/without debt, doctor warning, archive warning.

Verification: ./build.sh check; narrowed cargo test (single runs); live: finish a scratch task with a scratch worktree on dev 7686, observe close-time line + next nudge content + doctor warning, clean it, observe silence.
