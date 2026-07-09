---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_9d885f44614640e8990446cc2563d4b0
handle: chat-ui-command-rendering-ms_9d885f44614640e8990446cc2563d4b0
title: status_update
---

IN_REVIEW: STATUS: in_review
CHANGES: ba12833a — packages/chat-ui/src/MessageBubble.tsx, MarkdownBody.tsx, MessageBubble.providers.test.tsx, providerTranscriptFixtures.testdata.ts
BUILD/TYPECHECK/TESTS: npx tsx MessageBubble.providers.test.tsx (20/20), MarkdownBody.diff.test.tsx (5/5), MessageBubble.shell-command.test.tsx (7/7), recognize.test.ts (33/33). tsc has pre-existing plugin-sdk errors unrelated to lane.
FOLLOWUP: orchestrator should merge/gate visual screenshot — exec_command/Bash now render as full-width shell cards with command + result preview; apply_patch/generic tools get surface fill + diff padding.
