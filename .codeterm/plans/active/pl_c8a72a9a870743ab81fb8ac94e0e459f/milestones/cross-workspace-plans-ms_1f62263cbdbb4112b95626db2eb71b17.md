---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_1f62263cbdbb4112b95626db2eb71b17
handle: cross-workspace-plans-ms_1f62263cbdbb4112b95626db2eb71b17
title: status_update
---

Accepted and integrated cross-workspace plan UX slice.

Changes:
- Added derivePlanWorkspaceScope helper from existing workspace/tab and task lane metadata.
- CanvasView passes multiplexer workspaces into plan/spec node models.
- Plan cards show owner workspace on the owner chip and a cross-workspace badge when participating workspaces span more than one workspace.
- Added focused helper, card, and CanvasView tests.

Orchestrator gates:
- git diff --check: passed.
- npx tsx src/components/Canvas/planWorkspaceScope.test.ts: 3 passed.
- npx tsx src/components/Canvas/cards/cards.test.tsx: 20 passed.
- npx tsx src/components/Canvas/CanvasView.test.tsx: 9 passed.
- npx tsc --noEmit: passed.
- CODETERM_API_PORT=7686 ./build.sh check: passed.
- CT_E2E_PORT=7686 npx playwright test --config=tests/playwright/playwright.config.ts --project=golden --grep @canvas-multiview-overflow: 1 passed.

Residual risk:
- Dev daemon health build hash was stale, but this was frontend-only and the Playwright web-mode gate exercised the dev web client. No backend/schema changes.
- scripts/check-file-sizes.sh still reports unrelated pre-existing oversized files; touched files are under 1000 lines. CanvasView.tsx is now 984 lines and should not grow further.
