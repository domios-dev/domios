---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_7ea82b76f72e476ba598c43d3b29fbf2
handle: prompt-architecture-audit-tier-map-contradiction-sweep-prose-placement-ms_7ea82b76f72e476ba598c43d3b29fbf2
title: critical prompt audit finding
---

shipped docs still contradict prompt authority. docs/ORCHESTRATION what-lives-where says orchestration principle lives in mem skill codeterm-orchestrator; AGENTS.md says canonical orchestrator skill at ~/.codeterm/skills/codeterm-orchestrator; docs/PROMPTS.md says role definitions live in codeterm mem. Current prompt charter says docs/ORCHESTRATION + role prompts are authority, machine deltas in mem preferences, and role_guidance is inline. Also planner_suffix report template still says BUILD/TYPECHECK/TESTS instead of TESTS RUN/GATES REQUESTED. Fix before release with a minimal docs/prompt correction; no broad prompt architecture rewrite.
