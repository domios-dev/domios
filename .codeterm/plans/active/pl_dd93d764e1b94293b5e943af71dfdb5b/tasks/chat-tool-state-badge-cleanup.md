---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_3d4b9f5d36d044a8a4822ffd590bfd11
handle: chat-tool-state-badge-cleanup
title: Remove internal tool-state bubbles from chat messages
status: done
---

Remove noisy internal tool-state badges/bubbles from chat message rendering. User-visible problem: messages show labels like pending/result and similar implementation-state chips inside the chat transcript; these should not appear as decorative bubbles in normal chat. Keep useful tool content and command/result cards, but hide or restyle internal state markers so the transcript reads cleanly. Scope: packages/chat-ui message/tool rendering, likely MessageBubble/ToolOutputContent/provider fixtures. Do not break tool image previews or shell command cards. Use focused tests only.
