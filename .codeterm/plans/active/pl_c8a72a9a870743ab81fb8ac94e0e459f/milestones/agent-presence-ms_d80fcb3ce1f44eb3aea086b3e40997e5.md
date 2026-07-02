---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_d80fcb3ce1f44eb3aea086b3e40997e5
handle: agent-presence-ms_d80fcb3ce1f44eb3aea086b3e40997e5
title: status_update
---

Accepted agent-presence minimal lane. Canvas task cards now derive a presence marker from assigned_pane_id/task.lane.pane_id, live pane title lookup, plan heartbeat current_focus/active_lanes, and latest milestone author/kind. No backend/schema churn beyond frontend TS types matching existing plan JSON. Gates: npx tsx src/components/Canvas/taskPresence.test.ts passed; npx tsx src/components/Canvas/cards/cards.test.tsx passed (19 tests); npx tsx src/components/Canvas/CanvasView.test.tsx passed (8 tests); npx tsc --noEmit passed; CODETERM_API_PORT=7686 ./build.sh check passed; CT_E2E_PORT=7686 npx playwright test --config=tests/playwright/playwright.config.ts --project=golden --grep @content-lod passed; git diff --check passed. File-size guard: lane files under limits; script still reports unrelated pre-existing oversized files and src-tauri/src/mem/store/tests.rs baseline growth from other work.
