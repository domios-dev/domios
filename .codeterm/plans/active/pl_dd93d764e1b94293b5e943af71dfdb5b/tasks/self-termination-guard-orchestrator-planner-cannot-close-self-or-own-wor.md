---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_607fd5b6d0f447a68c74ae934d704e21
handle: self-termination-guard-orchestrator-planner-cannot-close-self-or-own-wor
title: 'Self-termination guard: orchestrator/planner cannot close self or own workspace'
status: pending
---

USER DIRECTIVE (round-101): forbid the ORCHESTRATOR from closing its own pane, and forbid the PLANNER from closing its own pane AND the workspace it works in — a coordinator that accidentally self-terminates (one wrong pane id in a cleanup loop) kills the whole orchestration silently. WALL-TIER GUARD, daemon-side: (1) 'pane close' (CLI/RPC/mesh — all paths) REFUSES when the caller pane (CODETERM_PANE_ID identity, the seam reports/spawner already use) equals the target pane AND the caller's role is Orchestrator or Worker+planner-profile: 'refusing: an orchestrator/planner cannot close its own pane; ask the user or a peer to close it'. (2) 'workspace close' REFUSES when the target workspace contains the caller pane and the caller is orchestrator or planner: same message shape naming the resident pane. (3) SCOPE PRECISION — the guard keys on ROLE, deliberately NOT on all agents: plain Workers and watcher panes self-closing is a DESIGNED pattern (TEXT D watcher tail 'codeterm pane close $CODETERM_PANE_ID') and must keep working — a guard that breaks watcher self-cleanup trades one bug class for another. (4) Human/UI close paths unaffected (frontend mux commands carry no agent caller identity — the guard only fires on agent-initiated CLI/RPC calls with a matching CODETERM_PANE_ID). (5) NO --force for agents — this is a hard wall; genuine teardown is the user's or a peer's action (a peer orchestrator or the GA closing a finished pair is fine — caller != target). Mesh relay path included (a remote self-close via --mesh loopback must also refuse).
