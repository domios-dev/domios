---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_cc1ed23bb66e424c8c624eca8c07eb34
handle: comment-hygiene-sweep-enforce-2-line-rule-refactor-code-out-of-its-comme
title: 'Comment hygiene sweep: enforce 2-line rule, refactor code out of its comments'
status: done
---

## What

Comment hygiene sweep, whole repo (user directive round-39): enforce the AGENTS.md rule — max 2 lines per comment, WHY not WHAT, and where a long comment compensates for unclear code, FIX THE CODE (rename, extract a function) so the comment becomes unnecessary. Doc comments (public API, file headers) stay but terse.

## Rules of the sweep

1. Delete: WHAT-comments (restating the next line), narrative blocks, stale references to tasks/fixes/phases, commented-out code.
2. Refactor-instead-of-comment: when a comment explains a confusing block, prefer renaming or extracting so the code says it — ONLY trivially-safe transforms (local rename, private fn extract, no signature/behavior changes). If a safe transform is not obvious, shorten the comment to <=2 WHY lines and move on.
3. Keep: terse WHY comments (invariants, OS quirks, deliberate workarounds), license headers, public-API docs (tersified if bloated).
4. Batch by directory (src-tauri/src/<module>, src/components/<family>), ONE COMMIT PER BATCH with focused tests run for that batch before committing. Skip references/, vendor/, pty-fork/, generated code.

## Executable test plan

1. METRIC before/after per batch: `grep -Pzo '(//[^\n]*\n){3,}'`-style multi-line comment-block count (script it once, attach counts) — after the sweep: zero 3+-line // blocks outside exempted doc comments (spot-audit 10 random survivors, justify each).
2. Per batch: focused tests for the touched module (scripts/test-capped.sh <filter> for Rust, npx tsx for TS), cargo check / tsc clean, file-size gate clean.
3. Behavior guard: zero test files' assertions changed; `git diff --stat` shows comments/renames only (spot-review the largest hunks).
4. Net LOC strictly negative per batch and total (numbers in the report).
