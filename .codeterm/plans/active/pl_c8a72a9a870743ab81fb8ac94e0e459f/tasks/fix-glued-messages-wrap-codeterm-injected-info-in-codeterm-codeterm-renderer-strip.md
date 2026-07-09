---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_1365d7ebb9b54a4ba9dec6e6b6d26978
handle: fix-glued-messages-wrap-codeterm-injected-info-in-codeterm-codeterm-renderer-strip
title: 'Fix glued messages: wrap codeterm-injected info in <codeterm>-=- -=-</codeterm> + renderer strip'
status: done
---

Prefix-builder: close the injected -=- block (delimiter at both ends, wrap in <codeterm>...</codeterm>). Chat renderer: detect+strip these blocks, render meta as a separate chip, so two messages don't merge.
