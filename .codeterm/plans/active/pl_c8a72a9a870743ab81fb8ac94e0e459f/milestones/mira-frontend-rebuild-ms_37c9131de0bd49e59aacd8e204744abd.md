---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_37c9131de0bd49e59aacd8e204744abd
handle: mira-frontend-rebuild-ms_37c9131de0bd49e59aacd8e204744abd
title: decision
---

§24 PRIORITY 1 done (7a4d5fd6). §24.1: removed Doc/Card/Tile mode switcher entirely — ONE behavior, every node adapts CONTENT to camera zoom (content-LOD) inside a FIXED frame; toolbar is camera-only. Deleted the manual-mode system (toolbar buttons, per-node mode state, resolveGlobalMode, NodeModeResolver, viewMode.ts). §24.3: UN-MERGED spec from plan (reverts the §22.2 merge) -> new cards/SectionNode.tsx (variant plan|spec) replacing SectionedPlanNode (+ old PlanNode/SpecSectionCard deleted). Three connected columns SPEC | PLAN | tasks: PLAN = one row/section (glyph·title·[n]·OUTPUT dot), SPEC = one block/section (content-LOD: far titles / near full prose w/ internal scroll), 1:1 spec<->plan edges by section id, plan OUTPUT -> banded tasks. Spec+plan share sectionNodeH so rows align. tsc clean, all canvas suites green (rewrote cards/layout/CanvasView/registry tests, deleted per-mode tests), vite clean, cargo+clippy pass. Next: §25 zoom-drill (Priority 2).
