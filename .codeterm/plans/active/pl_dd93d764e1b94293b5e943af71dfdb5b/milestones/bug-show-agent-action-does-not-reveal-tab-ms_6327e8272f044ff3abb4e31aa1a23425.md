---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_6327e8272f044ff3abb4e31aa1a23425
handle: bug-show-agent-action-does-not-reveal-tab-ms_6327e8272f044ff3abb4e31aa1a23425
title: status_update
---

STATUS: done
CHANGES: src/components/Canvas/planRevealAgentPane.ts (new), src/components/Canvas/planRevealAgentPane.test.ts (new), src/components/Canvas/CanvasView.tsx (wire onOpenPane through revealAssignedAgentPane)
BUILD/TYPECHECK/TESTS: CODETERM_API_PORT=7686 ./build.sh check OK; npx tsx src/components/Canvas/planRevealAgentPane.test.ts 5/5; npx tsx src/components/Canvas/planView/PlanDetailOverlay.test.tsx 7/7; git diff --check on touched files clean
FOLLOWUP: Playwright smoke skipped (no existing plan-board Show agent harness; unit coverage on reveal path). Manual verify: open plan board/list, task detail Show agent should close plan canvas overlay and activate the assigned agent tab.
