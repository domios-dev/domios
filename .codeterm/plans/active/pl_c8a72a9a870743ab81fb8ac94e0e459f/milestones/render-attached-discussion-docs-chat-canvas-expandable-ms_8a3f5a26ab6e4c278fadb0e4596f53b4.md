---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_8a3f5a26ab6e4c278fadb0e4596f53b4
handle: render-attached-discussion-docs-chat-canvas-expandable-ms_8a3f5a26ab6e4c278fadb0e4596f53b4
title: status_update
---

STATUS: done
CHANGES: packages/chat-ui/src/discussionDoc.ts, AttachedDiscussionDoc.tsx, MarkdownBody.tsx (renderUserContent), index.ts; src/components/Canvas/cards/MilestoneText.tsx, StrideSnake.tsx, MilestoneBubble.tsx, digest.ts, cards.test.tsx (StrideSnake bloom case), discussionDoc.test.ts
BUILD/TYPECHECK/TESTS: npx tsx packages/chat-ui/src/discussionDoc.test.ts 4/4 pass; ts-unit-tests.sh failed in worktree (no node_modules/react) — Orchestrator warm verify needed for React tests
FOLLOWUP: UX constraints noted (single Plan entry, no Tasks button; task-context opens Plan with camera on bound task; task-ref click fly/focus on full canvas) — out of scope. Sibling task task_f1db5434 externalizes milestone .md bodies to disk.
