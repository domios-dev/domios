---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_0a80b6ed163c439aa6881cc3d2b6f729
handle: migration-sweeps-surfaces-onto-shared-primitives-first-run-first
title: 'Migration sweeps: surfaces onto shared primitives, first-run first'
status: done
---

Migrate surface families onto the shared primitives, one sweep per family, each protected by before/after Playwright screenshots — intentional changes (the ugly first-run CTA becoming a real Button) are called out per-screenshot, everything else must be pixel-stable. ORDER: (1) FIRST-RUN + NEW-SESSION PICKER — Andrey's exhibit A: the 'Start General agent' text-link becomes <Button variant=primary size=lg>, provider cards become <Card> with proper spacing rhythm, the screen gets composition polish (this sweep doubles as the first-run visual fix he demanded); (2) Settings panels; (3) sidebar; (4) plan views; (5) chat chrome. Each sweep: swap markup to primitives, delete the bespoke classes, run the family's component tests + targeted Playwright, attach before/after pairs. Net LOC negative per sweep (measured in report). Executable test plan: per sweep — component tests green, targeted Playwright green, screenshot pairs attached with intentional-change annotations, LOC delta reported; grepgate allowlist shrinks after each sweep (numbers in report).
