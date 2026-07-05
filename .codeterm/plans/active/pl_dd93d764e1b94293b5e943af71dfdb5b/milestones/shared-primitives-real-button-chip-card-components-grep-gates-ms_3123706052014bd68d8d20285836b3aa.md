---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_3123706052014bd68d8d20285836b3aa
handle: shared-primitives-real-button-chip-card-components-grep-gates-ms_3123706052014bd68d8d20285836b3aa
title: status_update
---

IN_REVIEW: STATUS: in_review

CHANGES:
- src/components/shared/Button.tsx — primary/ghost/accent/danger/icon variants, sm/md/lg/compact sizes, loading (uses BusyState dots), disabled, aria-busy
- src/components/shared/Chip.tsx — default/accent/success/danger/warning/muted variants, wraps ct-small-pill
- src/components/shared/Card.tsx — default/soft variants, interactive=true adds role=button + keyboard handler
- src/components/shared/FormRow.tsx — label/hint/error states, htmlFor, role=alert on error
- src/components/shared/Button.test.tsx — 9 tests green
- src/components/shared/Chip.test.tsx — 6 tests green
- src/components/shared/Card.test.tsx — 6 tests green
- src/components/shared/FormRow.test.tsx — 6 tests green
- src/components/shared/shared.grepgate.test.ts — baseline-locked (chip=23/38, button=16/26), planted fixture verifies red path
- src/components/Gallery/ComponentGallery.tsx — full variant matrix, dev-only
- src/main.tsx — gallery route wired via ?prototype=gallery, lazy import, DEV only

BUILD/TYPECHECK/TESTS:
- npx tsc --noEmit: only 9 pre-existing plugin-sdk errors, zero new errors from my files
- npx tsx Button.test.tsx: 9/9 green
- npx tsx Chip.test.tsx: 6/6 green
- npx tsx Card.test.tsx: 6/6 green
- npx tsx FormRow.test.tsx: 6/6 green
- npx tsx shared.grepgate.test.ts: pass, planted fixture validates red path
- BusyState.grepgate.test.ts: still green
- Playwright gallery screenshot: dev not running in lane — run npx playwright test -g gallery after ?prototype=gallery opens in dev

FOLLOWUP:
- Orchestrator to run build gate (cargo check + npm run build)
- Migration sweeps (task_0a80b6ed): baseline counts in shared.grepgate.test.ts must only shrink as chips/buttons migrate
- Button loading uses BusyState inline=true with hideLabel — verify visually via gallery
