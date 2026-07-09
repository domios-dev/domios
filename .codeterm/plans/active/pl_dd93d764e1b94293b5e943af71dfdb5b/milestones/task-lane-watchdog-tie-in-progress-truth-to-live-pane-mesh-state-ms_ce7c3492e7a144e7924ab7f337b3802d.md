---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_ce7c3492e7a144e7924ab7f337b3802d
handle: task-lane-watchdog-tie-in-progress-truth-to-live-pane-mesh-state-ms_ce7c3492e7a144e7924ab7f337b3802d
title: review
---

SHARED-CHECKOUT RETRO (round-67, Andrey's question via Orchestrator: HOW was there no gate preventing workers editing the main checkout?). Honest answer, three compounding causes: (1) ISOLATION WAS PROSE — worktree-per-lane lived in AGENTS.md and task role_guidance; not one enforcement point existed in spawn, report, or daemon paths. Rules without affordances/enforcement lose to friction (same finding as the polling incident — this is now the plan's central lesson). (2) ALARM FATIGUE BY DESIGN — the daemon's own sidecar churn (.codeterm/plans/**) keeps every checkout permanently dirty, so the ONE existing guard (dirty-worktree report refusal) cried wolf constantly; workers learned to acknowledge/bypass, and real unrelated-dirty violations rode through the same door. The system trained the behavior it then suffered from. (3) ADOPTION DEBT — QA queue existed unused (3rd occurrence). PREVENTION SHIPPED AS TASKS: spawn guard task_46d7cc4c (Worker into canonical checkout refused without recorded exception — BEFORE RELEASE), sidecar churn task_4fc2ae89 (Phase 1 dirt-check exclusions BEFORE RELEASE + Phase 2 decision gate for Andrey on plans-in-git), report refusal hardening + code-only + build detection (task_5a6d58c5), worktree ownership visibility folded into plan lanes (task_d1450940 — added: flag two live lanes sharing one cwd as a collision). RELEASE VERDICT: 46d7cc4c + 4fc2ae89-Phase1 + 5a6d58c5 are release-gating (they make the remaining endgame gates trustworthy); dispatch/lanes commands stay post-release.
