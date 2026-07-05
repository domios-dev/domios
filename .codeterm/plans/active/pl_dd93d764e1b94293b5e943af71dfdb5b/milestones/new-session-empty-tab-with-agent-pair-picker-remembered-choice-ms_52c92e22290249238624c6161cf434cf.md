---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_52c92e22290249238624c6161cf434cf
handle: new-session-empty-tab-with-agent-pair-picker-remembered-choice-ms_52c92e22290249238624c6161cf434cf
title: status_update
---

IN_REVIEW: STATUS: in_review
CHANGES: merged origin/lane/peer-pair-visuals (PairAffordance); new src/lib/newSessionChoice.ts + sessionPickerPanes.ts + providerInstallStatus.ts; shared OrchestrationPairForm; NewSessionPicker in-tab UI; Sidebar New session -> empty picker tab; TabContent/App/multiplexer spawn+close wiring; globals.css picker pair layout. sha=1b91bb53
BUILD/TYPECHECK/TESTS: npx tsc --noEmit PASS; npx tsx newSessionChoice.test.ts 2/2; OrchestrationPairForm.test.tsx 2/2; OrchestrationCreate.test.tsx 5/5; git diff --check clean. No cargo/build/dev.
FOLLOWUP: Orchestrator UI gate — dev 7686: click sidebar New session -> empty tab shows picker with pair card (PairAffordance layout=card, equal Orchestrator/Planner rows); spawn single + pair; reopen New session and assert opus+fable preselect; close empty picker tab without spawn; update tests/playwright/suites/12-web/web-parity.spec.ts clickNewSession (no longer opens provider dropdown). Golden: npx playwright test --config=tests/playwright/playwright.config.ts --project=golden (new-session flow).
