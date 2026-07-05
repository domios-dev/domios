---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_bae1342b81314c959d6d3ada51f5c59b
handle: wave-27-design-system-enforcement-components-no-breakage
title: Wave 27 — Design system enforcement (components, no breakage)
---

Round-37: the first-run screen exposed the truth — UI_BUTTON/ct-btn tokens exist in shared/design.ts but 159 files render bespoke buttons; chips/cards/inputs same story. The wave: audit the spread, fill primitive gaps as real components, migrate surface-by-surface under visual-regression protection, and grep-gate the future (the BusyState.grepgate precedent). Net-negative diff, nothing breaks.
