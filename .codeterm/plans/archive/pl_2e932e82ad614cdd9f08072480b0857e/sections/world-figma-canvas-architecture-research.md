---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: section
id: sec_f6e82e3fdad64b6294ba9cbfd4617a5b
handle: world-figma-canvas-architecture-research
title: World/Figma canvas architecture research
---

Current direction: stop iterating on Worlds v3 as a DOM/CSS zoom path. The visible bug is architectural: nodes/cards shrink, wobble, and reflow while approaching because the old path relies on per-card web/CSS tricks instead of a stable canvas camera/substrate model. Research the right World/Figma-like architecture before implementation: camera matrix, object-space layout, crisp text, rich markdown/mermaid content, hit-testing, focus-by-id, progressive worlds/LOD, weak-machine performance, and an incremental migration path. Prior S0/S1 findings are input evidence, not the final answer.
