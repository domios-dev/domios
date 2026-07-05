---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_bb09a057331548b4a25c7b43f5996cdd
handle: bug-worker-preamble-teaches-plan-record-step-decision-text-but-cli-requires-decision-text-colon-every-worker-stride-fails
title: 'BUG: worker-preamble teaches ''plan record --step decision|text'' but CLI requires ''decision:text'' (colon) — every worker stride fails'
status: done
tags:
- friction
---

worker-preamble.md example: 'plan record ... --step "decision|<what>"'. CLI rejects pipe: 'invalid --step; expected kind:text or kind:@file'. The separator is a COLON. Fix the preamble example (and/or accept pipe). Otherwise every worker's stride-recording silently errors.
