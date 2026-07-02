---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_bff6b9c0833f4d5b9601163b0ee5de63
handle: workspace-plan-entry-visual-treatment-and-empty-state-policy-ms_bff6b9c0833f4d5b9601163b0ee5de63
title: testing
---

Final orchestrator correction after user UX review:
- Workspace Plan entry is project-local only; foreign project plans no longer leak into General through owner/assigned pane fallbacks.
- Visible label is stable "Plan"; plan names/counts move to title/metadata, not primary row text.
- Empty/projectless workspaces hide the Plan action.
- Workspace Plan canvas no longer shows Close X; it behaves as a workspace view.
- Clicking the workspace tab row closes the Plan canvas.

Final gates:
- npx tsx src/components/Canvas/workspacePlanEntry.test.ts: 16/16 passed.
- npx tsc --noEmit: passed.
- env CODETERM_API_PORT=7686 ./build.sh check: passed.
- pnpm build: passed.
- npx playwright test --config=tests/playwright/playwright.config.ts --project=golden --grep @workspace-plan-entry --reporter=line --timeout=60000: 1 passed.
