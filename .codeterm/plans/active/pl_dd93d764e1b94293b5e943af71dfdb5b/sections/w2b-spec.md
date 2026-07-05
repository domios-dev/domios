---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_69fb9e16b4bc48788d1bc03a1e371305
handle: w2b-spec
title: Wave 2b — Spec presentation redesign
---

Wave 2b (parallel with Wave 2, shares the detail surface): redesign how the spec ENTITY reads — the canvas spec card and the spec read surface.

Scope:
- Canvas spec card content model (canvasRender/cardContent.ts, drawCard.ts): replace truncated per-section rows + identical "→ tasks" chips with a compact summary (title, section count, coverage rollup) and readable top-level section titles; per-row coverage glyph/count only when it carries signal.
- Spec detail in the shared overlay: full untruncated section tree, bodies expandable, per-section coverage (linked plan sections, task counts/status) — implemented against PlanDetailOverlay from Wave 2.
- Board/List reach the same spec detail through PlanContextHeader (Wave 3 dependency for the header itself; the detail content ships here).

File ownership: canvasRender/cardContent.ts + cardContent.test.ts, canvasRender/drawCard.ts + drawCard.test.ts, canvasRender/DetailContent.tsx (spec branch), planView/PlanDetailOverlay.tsx (spec kind).
Verification: npm run test:unit; visual screenshot of the spec card + spec detail in Wave 6 evidence set.
