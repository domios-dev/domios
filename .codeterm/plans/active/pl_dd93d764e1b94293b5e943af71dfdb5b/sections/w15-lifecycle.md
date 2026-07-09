---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_13dda21e55e3432d93dba695a76d810b
handle: w15-lifecycle
title: Wave 15 — Task lifecycle + relentless heartbeat
---

Wave 15 (rust, task lifecycle + heartbeat): PARKED status with mandatory reason (working-point stride), unpark transition; replan-handoff exit documented in nudge text; heartbeat continues while ANY task is pending/in_progress/in_review/blocked and says so explicitly; age-threshold escalation (stale open tasks named in the nudge with the five allowed exits: dispatch/park/judge/replan/escalate-model); drift extension beyond stale_in_review; doctor: error on parked-without-reason, warning on aged unstaffed open tasks.
Areas: service/plan status model + transitions (mod.rs/mutations.rs), heartbeat.rs nudge text + escalation, state.rs drift, doctor rules, CLI status arg + park command surface, ORCHESTRATION doc.
Verification: narrowed cargo tests (transitions, nudge text, escalation, doctor); live scratch-plan smoke: open task ages past threshold -> escalated nudge names it; park without reason rejected.
