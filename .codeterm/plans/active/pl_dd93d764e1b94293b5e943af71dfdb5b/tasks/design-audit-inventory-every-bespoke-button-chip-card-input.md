---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_73799ebbdcfa4926862b16ca8ff58a4c
handle: design-audit-inventory-every-bespoke-button-chip-card-input
title: 'Design audit: inventory every bespoke button/chip/card/input'
status: done
---

Read-only audit across src/components (159 files render buttons alone): for every interactive primitive (button, chip/badge, card, input/select row) classify each usage — (a) uses shared tokens (UI_BUTTON/ct-btn family, DropdownMenu, Segmented), (b) bespoke duplicate of an existing primitive, (c) genuinely custom (justify). OUTPUT: a migration table (file -> element -> class -> target primitive -> risk note) attached as a doc, plus a GAP LIST of missing variants/components the tokens do not cover yet (e.g. the first-run CTA needs a large primary Button; card container; form row). Include a screenshot atlas of the worst offenders (the first-run screen is exhibit A). No code changes. Executable test plan: the deliverable IS the table — verify coverage by 'grep -c button' per directory reconciled against table row counts (numbers must match, attach the reconciliation).
