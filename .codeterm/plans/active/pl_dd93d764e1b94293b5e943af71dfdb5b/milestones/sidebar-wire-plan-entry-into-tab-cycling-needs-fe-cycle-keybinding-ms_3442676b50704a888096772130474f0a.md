---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_3442676b50704a888096772130474f0a
handle: sidebar-wire-plan-entry-into-tab-cycling-needs-fe-cycle-keybinding-ms_3442676b50704a888096772130474f0a
title: status_update
---

IN_REVIEW: STATUS: in_review
CHANGES:
  - src/lib/types.ts: added next_tab/prev_tab to KeybindingsConfig
  - src/components/Sidebar/sidebarCycle.ts (NEW): buildSidebarCycleItems + cycleStep pure logic
  - src/components/Sidebar/sidebarCycle.test.ts (NEW): 14 node:assert tests, all pass via npx tsx
  - src/components/Sidebar/Sidebar.tsx: global keydown useEffect for Ctrl+Tab / Ctrl+Shift+Tab cycling
  - src/components/Settings/SettingsPanel.tsx: next_tab/prev_tab visible in Keybindings settings
  - src/help/content/en.ts: next/prev tab entries in shortcut table
  - docs/CONFIG.md: annotated next_tab/prev_tab with plan entry cycling note, bumped revision to 24

BUILD/TYPECHECK: npx tsc --noEmit — zero errors in production code (plugin SDK import errors are pre-existing)
TESTS: npx tsx src/components/Sidebar/sidebarCycle.test.ts → ok (14 assertions)

DESIGN:
- Cycle order: plan entry (if workspace has active plans) first, then flattenSidebarTabs order
- Selection XOR invariant preserved: uses workspaceCanvasActive prop (same derivation as isWorkspacePlanEntryActive) — no new flags
- To plan: onNavigateToTab() to close canvas + setMainView(workspace), then openWorkspacePlanForEntries() to reopen
- To tab: setActiveTab() + onNavigateToTab() — identical to sidebar click behavior
- Keybinding defaults: Ctrl+Tab forward, Ctrl+Shift+Tab backward (config-driven via e.code)
- Cycle skips if < 2 items (single tab, no plan = no-op)

FOLLOWUP: orchestrator should build/verify; no Playwright test added (canvas cycling requires desktop env)
