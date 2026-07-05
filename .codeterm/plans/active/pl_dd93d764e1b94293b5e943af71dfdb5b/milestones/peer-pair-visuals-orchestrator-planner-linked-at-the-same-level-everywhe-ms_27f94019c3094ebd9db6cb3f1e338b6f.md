---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_27f94019c3094ebd9db6cb3f1e338b6f
handle: peer-pair-visuals-orchestrator-planner-linked-at-the-same-level-everywhe-ms_27f94019c3094ebd9db6cb3f1e338b6f
title: status_update
---

IN_REVIEW: STATUS: in_review
CHANGES: src/components/shared/PairAffordance.tsx, resolvePlanPeerPair.ts (+tests); Sidebar.tsx (peer pair tab group + planner excluded from HierarchyTree); HierarchyTree.tsx (excludePaneIds); PlanContextHeader.tsx (chips via PairAffordance); PlanCompanionBubble.tsx (peer-pair tooltip); globals.css (pair-affordance styles). sha=2ee49bf82dc1ffef9bbb1c9f6309edf335e01453
BUILD/TYPECHECK/TESTS: npx tsx resolvePlanPeerPair.test.ts, PairAffordance.test.ts, HierarchyTree.test.ts — pass; npx tsc --noEmit — no src/components errors (pre-existing plugin-sdk noise only); git diff --check — pass; no cargo/build/dev
FOLLOWUP: PairAffordance export: import from src/components/shared/PairAffordance.tsx. Props: { layout: 'sidebar' | 'chips' | 'card'; orchestrator: ReactNode; planner: ReactNode; className?: string; 'data-testid'?: string }. Helpers re-exported: resolvePlanPeerPair, resolvePlannerPaneIdFromPlan, findOwnedPlanForOrchestrator. Picker lane (task_685b5166): use layout="card". Orchestrator should gate screenshots on dev with live orchestration pair. Companion bubble done; picker not touched.
