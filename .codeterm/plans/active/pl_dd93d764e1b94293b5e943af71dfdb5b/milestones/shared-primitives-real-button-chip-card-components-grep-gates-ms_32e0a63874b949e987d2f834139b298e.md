---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_32e0a63874b949e987d2f834139b298e
handle: shared-primitives-real-button-chip-card-components-grep-gates-ms_32e0a63874b949e987d2f834139b298e
title: report_refused
---

PARTIAL: REPORT NOT SENT
readiness report refused: this git worktree has uncommitted lane changes.
Next commands:
  git status --short
  git add <files>
  git commit -m "<message>"
  codeterm agent report --status in_review --sha $(git rev-parse HEAD) "..."
Or report WIP with:
  codeterm agent report --status partial "..."
Dirty lane files:
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/active-work.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/canonical-sidecar.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/cleanup-hygiene.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/competitive-execution.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/id-ergonomics.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/loop-mechanics.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/mission-control-canvas-requirements-round-8.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/p0-report-delivery-dirty-worktree.md

Original readiness report that was refused locally:
STATUS: in_review

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
- src/main.tsx — gallery route wired (?prototype=gallery, lazy import, DEV only)

BUILD/TYPECHECK/TESTS:
- npx tsc --noEmit: only 9 pre-existing plugin-sdk errors, zero new errors from my files
- npx tsx Button.test.tsx: 9/9 green
- npx tsx Chip.test.tsx: 6/6 green
- npx tsx Card.test.tsx: 6/6 green
- npx tsx FormRow.test.tsx: 6/6 green
- npx tsx shared.grepgate.test.ts: pass, planted fixture validates red path
- BusyState.grepgate.test.ts: still green (unmodified)
- Playwright gallery screenshot: dev not running in this lane — command is: npx playwright test -g 'gallery' --project=golden after opening ?prototype=gallery in dev

FOLLOWUP:
- Orchestrator to run build gate (cargo check + npm run build pass)
- Gallery Playwright screenshot can be captured once dev is rebuilt
- Migration of bespoke chips to Chip primitive (task_0a80b6ed) is the next wave — baseline counts in shared.grepgate.test.ts must only shrink as that work lands
- Button loading drops BusyState inline=true with hideLabel — visually the dots appear in the button; orchestrator can verify this visually via gallery route
- All files <130 lines, well under 1000-line limit
