---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_d6d217fd276a4d6d98bc8664f35fb162
handle: tab-title-stability-no-auto-rename-ms_d6d217fd276a4d6d98bc8664f35fb162
title: status_update
---

STATUS: partial
CHANGES: src/hooks/terminal/useAgentDetection.ts, src/hooks/terminal/useShellIntegration.ts, src/store/multiplexer/reducer.ts, src/store/multiplexer/reducer.test.ts (branch also has prior TabItem/reducer/tree fixes)
BUILD/TYPECHECK/TESTS: reducer tests not run (tsx missing locally); ./build.sh check started but interrupted before finish
FOLLOWUP: alive; current state: working — passive OSC/CWD title rename removed, SET_STATE merge guards frontend Auto; need to finish build.sh check + run reducer tests via npx tsx
