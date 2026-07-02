---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_c68522dbb4274ffeb0d1c488d381e6c2
handle: audit-planner-cli-error-explanations-ms_c68522dbb4274ffeb0d1c488d381e6c2
title: status_update
---

Accepted. Read-only audit found missing/weak planner CLI error renderers: bad_mode, bad_priority, context-aware not_found and bad_status, io, orchestrator handoff/status non-plan RPC bypasses, plus a guard against unknown structured PlanError types falling back to raw RPC. Known transition_denied issue is covered by the current patch.
