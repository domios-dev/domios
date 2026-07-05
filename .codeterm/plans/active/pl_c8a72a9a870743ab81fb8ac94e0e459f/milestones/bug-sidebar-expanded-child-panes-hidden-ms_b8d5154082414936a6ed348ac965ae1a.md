---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_b8d5154082414936a6ed348ac965ae1a
handle: bug-sidebar-expanded-child-panes-hidden-ms_b8d5154082414936a6ed348ac965ae1a
title: status_update
---

Accepted and integrated into feat/canvas.

Changes:
- Sidebar workspace tab helpers now distinguish visible pane ids from tab membership, preserve child nesting when child cwd differs from workspace cwd, and surface children of hidden parents at top level instead of losing them.
- Compact sidebar mode now uses depth-first `flattenSidebarTabs` instead of raw tab sort, so backend child panes are reachable in compact expanded workspace lists too.
- Added focused helper coverage for cwd-mismatched children, grandchildren, and hidden-parent surfacing.

Orchestrator verification in /Users/rollacode/Developer/codeterm:
- npx tsx src/components/Sidebar/workspaceTabs.test.ts (passed)
- npx tsc --noEmit -p tsconfig.json (passed)
- git diff --check for touched files
- bash scripts/check-file-sizes.sh (only known unrelated baseline files; touched files under limits, Sidebar.tsx 932 lines)
- CT_E2E_PORT=7686 npx playwright test --config=tests/playwright/playwright.config.ts --project=golden --grep "C2 a child reorders" (1 passed)

Residual:
- Live prod UI will need a build/restart/release to pick up this frontend code; dev web-mode passed the hierarchy reorder gate.
