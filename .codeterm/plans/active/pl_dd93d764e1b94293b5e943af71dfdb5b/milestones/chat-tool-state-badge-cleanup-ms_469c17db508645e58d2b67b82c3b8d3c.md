---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_469c17db508645e58d2b67b82c3b8d3c
handle: chat-tool-state-badge-cleanup-ms_469c17db508645e58d2b67b82c3b8d3c
title: status_update
---

STATUS: done
CHANGES: packages/chat-ui/src/MessageBubble.tsx (removed pending/running/result phase badge from curated chatBackend tool cards), packages/chat-ui/src/MessageBubble.providers.test.tsx (updated exec card assertion + new lifecycle badge regression test)
BUILD/TYPECHECK/TESTS: npx tsx packages/chat-ui/src/MessageBubble.providers.test.tsx (23/23 pass), npx tsx packages/chat-ui/src/MessageBubble.shell-command.test.tsx (7/7 pass), npx tsx packages/chat-ui/src/toolImageOutput.test.ts (5/5 pass), git diff --check (clean). Broad build/tsc not run per task scope.
FOLLOWUP: Shell command cards (ShellCommandItem) and generic tool_call cards unchanged — still show useful content/result previews. Visual screenshot gate left to orchestrator if desired.
