---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_41669c2abd5b4566b059dbb0999e9af8
handle: chat-render-outgoing-codeterm-send-as-a-real-sent-message-bubble
title: 'Chat: render outgoing ''codeterm send'' as a real sent-message bubble'
status: done
---

USER INTENT (verbatim spirit): when an agent sends a message to another pane, the chat must show it 'по красоте' — as a visually rendered SENT MESSAGE in the conversation ('он им это отправил'), not as a command card with one grey truncated line. CURRENT STATE (exact leads): packages/chat-ui/src/codeterm/recognize.ts detects codeterm CLI invocations; templates.ts:39 sendTextTemplate renders send as {icon '→' or mesh '🛰', title 'Sent to <pane-name> @ <mesh>', detail = first positional truncated}; CodetermCard.tsx renders that as a compact one-liner. REQUIRED: a dedicated SentMessageBubble rendering for the 'send' template family: (1) looks like an OUTGOING chat message — bubble styled in the same family as user messages but visually distinct (subtle accent tint/left accent), so the eye reads 'a message was sent', not 'a command ran'; (2) recipient header: '→ <resolved tab name> @ <mesh peer>' with the existing PaneNameResolver + provider glyph when resolvable; delivered/failed status from deliveredStatus() as a quiet check/cross suffix, not a badge box; (3) FULL message text rendered as markdown (same renderer as normal bubbles — these messages carry structure: lists, headers), collapsed beyond ~8 lines with an expand affordance (reuse the existing card expand pattern); (4) mesh and local sends both covered; multi-send bursts stack cleanly; (5) keep recognize/templates tests green, add component tests: send with long markdown body renders bubble + collapses; failed send shows failure state; pane-name resolution fallback ('pane 915fae8e' when unresolvable). Design-system note: use shared components/tokens (Wave 27 primitives where they exist) — no new bespoke button/chip.
