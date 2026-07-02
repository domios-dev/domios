---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_b24af6854ad94ae58a0795ae8c124c98
handle: plan-goal-north-star-ms_b24af6854ad94ae58a0795ae8c124c98
title: decision
---

HANDOFF VERIFY-LOOP (extends §27): compose (orchestrator handoff --verify) → show --latest --markdown read-back → self-verify vs MAIN SPEC ×3 (independent passes) → only then compact/reseed → fold into SKILL.md on live proof. The 5 spec-completeness checks per pass: GOAL (bullets match Plan.goal.vector+constraints), STATE (plan_snapshot==plan get, not stale), IN-FLIGHT (every active worker/pane/branch/session captured vs agent reports+branch ledger), NEXT-ACTIONS (first 3 executable w/o chat context, real ids), RULES (foundational_rules+enabled postulates present). Any gap → amend + re-run; 3 clean passes = verified.
