---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_fd6ee217315b4969833cfcdce96f16f4
handle: bug-chat-image-output-base64-ms_fd6ee217315b4969833cfcdce96f16f4
title: accepted
---

chat image output fix integrated as fb19d277. Gates: toolImageOutput 5/5, MessageBubble.providers 22/22, git diff --check clean. Raw image base64 now renders as bounded preview. Residual: MessageBubble.tsx remains oversized and is tracked by t23-file-size-gate.
