---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_effdb8024c1c479c953015beb8ba6701
handle: branch-lane-lifecycle-standard-canonical-doc-naming-enforcement-stale-sw-ms_effdb8024c1c479c953015beb8ba6701
title: decision
---

FIX-WHERE-THE-GATE-LIVES RULE (round-87, Andrey via Orchestrator — clarifying addition to the lifecycle standard, also recorded against the QA task's registry design): for tight RED->small-fix->rerun loops, when a remote machine owns the relevant warm worktree/gate AND the fix is local to that gate's findings, spawn a REAL CodeTerm worker ON that machine (workspace + agent spawn --mesh, bound to the task, lane branch fetched there) to fix and rerun that machine's single QA gate in place — instead of ping-ponging text through the orchestrator and a local worker (each round-trip costs a full report/redispatch cycle for a one-line fix). ALL INVARIANTS PRESERVED, explicitly: one heavy gate per physical machine (the remote worker uses THAT machine's registered gate, nothing new); real worker panes only, never ad-hoc chat; branch/task binding visible (lane fields set at spawn); orchestrator retains merge/accept authority — the remote worker reports in_review like any lane, it just iterates against the gate locally. DECISION HEURISTIC for the doc: fix-loop lives WHERE THE REPRO LIVES — local repro -> local worker; remote-only repro/gate -> remote worker; cross-machine fixes (code here, verify there) -> local worker + remote gate routing as before.