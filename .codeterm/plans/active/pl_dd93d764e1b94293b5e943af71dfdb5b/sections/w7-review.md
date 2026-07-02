---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_5add1d7ac9ca45af9f9bf13e5e745cd8
handle: w7-review
title: Wave 7 — Integration, adversarial review, approval
---

Wave 7: Integration, adversarial review, and approval gate.

Scope:
- Integrate all lane branches into the integration checkout (feat/canvas-updates or successor branch); resolve conflicts; run the full gate: npm run lint, npm run test:unit, scripts/check-file-sizes.sh, npx playwright test -c tests/playwright --project=golden, ./build.sh check.
- Adversarial code review of the combined diff with explicit lenses: (a) UX regressions in flows NOT redesigned (canvas presence, fly-to, composer interactions), (b) perf on hot paths (no per-render fetches, PlansProvider churn, canvas frame budget), (c) file-size/style discipline, (d) testid/e2e contract stability, (e) theme-token correctness in both light/dark.
- Self-review checklist against every acceptance gate in the spec (gates-req section); each gate gets an explicit pass/fail line with evidence.

Approval: the Orchestrator holds final acceptance; visual "pleasant" judgment escalates to the user with the Wave 6 screenshots. Any gate failure returns the owning task to in_progress with a concrete bounce note, or escalates to the Planner (Wave 8) if the failure is structural.
