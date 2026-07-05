---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_4a2a572e31b247aa843ad361bced1429
handle: fix-glued-messages-wrap-codeterm-injected-info-in-codeterm-codeterm-renderer-strip-ms_4a2a572e31b247aa843ad361bced1429
title: testing
---

ACCEPTED on review. Both-sides, backward-compatible: message_markers.rs builds <codeterm>-=-codeterm:kind...-=-</codeterm> envelope + strips it; chatPrefixes.ts splits both wrapped+bare markers; chat-core markers.ts + tests, messenger.rs, delayed_send.rs. Comprehensive unit tests added (message_markers_tests +45, markers.test, chatPrefixes.test). @cf79d47c on canvas/glued-msgs. NOTE: full Tauri build test couldn't run in worktree (build.rs needs FE dist — env, not code); marker unit tests run at mainline integration.
