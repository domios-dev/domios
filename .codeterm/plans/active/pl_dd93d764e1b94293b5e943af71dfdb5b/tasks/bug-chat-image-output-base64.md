---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_bf850fed7b3840bba2140c00492207bb
handle: bug-chat-image-output-base64
title: 'Bug: chat image tool output renders giant base64'
status: done
---

Fix chat message rendering for image/tool outputs. When a tool result or markdown/code payload contains a data:image/*;base64 or equivalent image artifact, do not render the raw base64 text. Show a bounded image preview/attachment (max height/width, object-fit contain) or a compact collapsed placeholder with filename/type/size and an explicit expand/download action. Preserve normal text and code rendering for non-image tool outputs. Add tests with a realistic data:image base64 payload to prevent giant raw output regressions.
