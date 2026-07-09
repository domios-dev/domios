---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_420d97eac7c34d3290c9d167c5ce4c99
handle: render-attached-discussion-docs-chat-canvas-expandable-ms_420d97eac7c34d3290c9d167c5ce4c99
title: status_update
---

STATUS: done
CHANGES: packages/chat-ui/src/discussionDoc.ts (parse agent --doc embed), AttachedDiscussionDoc.tsx (expandable card), MarkdownBody renderUserContent + index exports; src/components/Canvas/cards/MilestoneText.tsx, StrideSnake.tsx, MilestoneBubble.tsx, digest.ts (summary-only digest for snake labels); tests: discussionDoc.test.ts, cards.test.tsx StrideSnake bloom case
BUILD/TYPECHECK/TESTS: npx tsx packages/chat-ui/src/discussionDoc.test.ts — 4/4 pass; React component tests not run (worktree has no node_modules/react)
FOLLOWUP: UX constraints noted (no separate Tasks view/button; single Plan entry with optional initial camera on bound task; task-ref click should fly/focus on full canvas) — out of scope for this task. Remaining: externalize milestone .md bodies to disk (sibling task task_f1db5434), wire task-ref links in discussion markdown to canvas focus/fly.
