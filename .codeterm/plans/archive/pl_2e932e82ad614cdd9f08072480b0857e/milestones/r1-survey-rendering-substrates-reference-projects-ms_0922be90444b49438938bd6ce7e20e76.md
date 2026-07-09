---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: milestone
id: ms_0922be90444b49438938bd6ce7e20e76
handle: r1-survey-rendering-substrates-reference-projects-ms_0922be90444b49438938bd6ce7e20e76
title: decision|VERDICT
---

hybrid but inverted — DOM-world + single CSS-transform camera (tldraw model) as primary substrate; thin GPU/Canvas2D layer only for ephemeral overlays + far-LOD labels. NOT pure-GPU-world+DOM-overlay. Invariant met by ONE container transform (no per-card transform = no reflow/re-raster); markdown(HTML)+mermaid(SVG) native. Weak-HW bounded by LOD content-swap + culling. Escalate to PixiJS+SDF for far bands only if S1 spike misses fps budget.
