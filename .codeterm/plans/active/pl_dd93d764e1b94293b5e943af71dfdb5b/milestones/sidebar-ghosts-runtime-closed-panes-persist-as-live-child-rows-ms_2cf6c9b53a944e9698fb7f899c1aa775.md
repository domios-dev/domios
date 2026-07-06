---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_2cf6c9b53a944e9698fb7f899c1aa775
handle: sidebar-ghosts-runtime-closed-panes-persist-as-live-child-rows-ms_2cf6c9b53a944e9698fb7f899c1aa775
title: review
---

Diff reviewed: small daemon prune of dead delegated Worker panes in pty_lifecycle plus hierarchyRuntimeClose regression. Frontend test passed with main node_modules: npx tsx hierarchyRuntimeClose.test.ts. Rust cargo check attempted with TEST_CAP_SECS=180 and capped during cold dependency compile before CodeTerm crate; not accepted yet, needs warm QA/builder cargo check before merge.
