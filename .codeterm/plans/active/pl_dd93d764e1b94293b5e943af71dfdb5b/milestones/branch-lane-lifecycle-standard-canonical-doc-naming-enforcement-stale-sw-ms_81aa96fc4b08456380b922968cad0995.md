---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_81aa96fc4b08456380b922968cad0995
handle: branch-lane-lifecycle-standard-canonical-doc-naming-enforcement-stale-sw-ms_81aa96fc4b08456380b922968cad0995
title: decision
---

GENERALIZATION CORRECTION (round-69, user: CodeTerm is a shipped orchestration product — the standard must work for ANY project with branching, not for developing codeterm itself). SUPERSEDES the doc-home in the body: the canonical home of the lifecycle standard is an EMBEDDED PRODUCT DOC — new tier-3 doc served as 'codeterm docs LANES' (include_str like ORCHESTRATION.md, add to the docs registry + the orphan-loader test from task_52539b4e), NOT docs/contributing/lanes.md. The worker_suffix pointer line becomes 'Lane lifecycle: codeterm docs LANES' — resolvable in EVERY project the product runs in. docs/contributing/lanes.md in this repo becomes a thin pointer for human contributors (one paragraph + 'run codeterm docs LANES'). CONTENT RULE for the embedded doc: zero references to this repo's specifics — no feat/canvas-updates, no build.sh, no scripts/test-capped.sh; the standard speaks in product concepts only (integration head, task short-id, plan qa, worktrees). Verify every other task in this process wave for the same leak: spawn guard, naming check, code-only detection, auto-cleanup are all product code (already general); the ONLY repo-local artifact allowed is the thin contributor pointer.
