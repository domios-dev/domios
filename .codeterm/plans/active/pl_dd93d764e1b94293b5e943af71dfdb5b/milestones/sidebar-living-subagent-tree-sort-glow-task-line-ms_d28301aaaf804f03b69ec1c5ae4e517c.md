---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_d28301aaaf804f03b69ec1c5ae4e517c
handle: sidebar-living-subagent-tree-sort-glow-task-line-ms_d28301aaaf804f03b69ec1c5ae4e517c
title: status_update
---

STATUS: in_review
CHANGES: src/components/Sidebar/HierarchyTree.tsx, HierarchyTree.test.ts, sortHierarchyNodes.ts, sortHierarchyNodes.test.ts, Sidebar.tsx (wire HierarchyTree under orchestrator tabs), src/styles/globals.css, .codeterm/artifacts/sidebar-hierarchy-working-variants.html
BUILD/TYPECHECK/TESTS: npx tsc --noEmit PASS; npx tsx src/components/Sidebar/sortHierarchyNodes.test.ts PASS; npx tsx src/components/Sidebar/HierarchyTree.test.ts PASS; npx tsx src/components/Sidebar/TabItem.test.ts PASS; git diff --check PASS
FOLLOWUP: Branch lane/sidebar-liveliness committed at e2b6a8b3 but git push blocked by pre-push cargo check (missing ../dist in worktree). Orchestrator should push from warm checkout or build dist first. Working-row variants: aurora (default, radial breathe), drift (horizontal sweep), halo (inset glow) — static demo at .codeterm/artifacts/sidebar-hierarchy-working-variants.html. Chose aurora as default; orchestrator ★ kept on orchestrator rows only; gear+StatusDot removed from worker rows; Dead=55% opacity+red title+✕; Waiting reads via task line. Playwright golden gate not run (lane-local scope). QA: expand orchestrator in live dev to verify sort glide + task line with real bound workers.
