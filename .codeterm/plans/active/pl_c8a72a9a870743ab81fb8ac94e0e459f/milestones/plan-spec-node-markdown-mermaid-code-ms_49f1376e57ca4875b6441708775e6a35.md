---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_49f1376e57ca4875b6441708775e6a35
handle: plan-spec-node-markdown-mermaid-code-ms_49f1376e57ca4875b6441708775e6a35
title: status_update
---

Integrated plan-spec-rich-content: Plan/Spec section bodies now delegate through a SectionMarkdownBody wrapper with data-renderer=rich-markdown and focused coverage for markdown headings, code blocks, copy affordance, and mermaid rendering. Verified SectionNode.richContent.test.tsx 2/2, cards.test.tsx 18/18, npx tsc --noEmit, and CODETERM_API_PORT=7686 ./build.sh check.
