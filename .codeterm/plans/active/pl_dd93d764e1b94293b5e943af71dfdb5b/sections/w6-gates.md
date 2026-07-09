---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_85b0ef9d151b47d386a1da4a33e1cc6e
handle: w6-gates
title: Wave 6 — Visual and e2e verification gates
---

Wave 6: Visual and e2e verification gate (Playwright web-mode on macOS against dev 7686). Runs after Waves 1-5 land in the integration checkout.

Scope:
- Update/extend tests/playwright/suites/13-canvas/plan-view-mode.spec.ts: Board is default, switch order Board/List/Canvas, mode persistence across plan close/reopen.
- New specs: detail overlay opens from Board card, List row, and Canvas card, Esc restores state; Done column collapsed by default with working "show all"; List done-only sections collapsed with rollups.
- Done-heavy fixture: drive the perfFixture (200 done + 10 active) and assert bounded board height / collapsed list.
- Screenshot artifacts: capture Board, List, Canvas on both normal and done-heavy fixtures to tests/playwright/artifacts/ and attach paths in the task report for human visual acceptance.

Verification commands: npx playwright test -c tests/playwright --project=golden (plus targeted spec runs); npm run test:pw:lint for tag hygiene.
Gate contract: this wave produces the evidence the Orchestrator forwards to the user for the "pleasant" judgment; a red or unreadable screenshot is a bounce back to the owning wave, and repeated bounces go back to the Planner as replanning input.
