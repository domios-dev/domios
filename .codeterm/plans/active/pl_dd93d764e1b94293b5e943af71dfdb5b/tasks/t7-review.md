---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_53b32413cc504e24aa42d6942d07b3c5
handle: t7-review
title: Adversarial review + integration gate
status: done
---

Adversarial review + integration gate over the combined plan-views redesign diff. You judge against the spec's acceptance gates (codeterm spec show sp_d2c70550432f4a829c89cf4a4cb7c3c5, section "Visual and functional acceptance gates").

Review lenses (each gets explicit findings):
a. UX regressions in untouched flows: canvas presence layer, fly-to focus, composer interactions, multi-plan workspace mode.
b. Perf: no per-render fetches added (PlanBoardView/PlanListView spec fetching), PlansProvider churn, canvas frame budget claims verified against reported numbers.
c. Discipline: scripts/check-file-sizes.sh clean, npm run lint clean, comments per repo style (max 2 lines, WHY only), --ct-* tokens only (both themes sane).
d. Contract stability: testids preserved/documented, PlanDetailOverlay API not leaking canvas types into planView/.
e. Gates: run the full gate suite and record per-gate pass/fail with evidence.

Commands: npm run lint; npm run test:unit; bash scripts/check-file-sizes.sh; npx playwright test -c tests/playwright --project=golden; ./build.sh check.
Report: per-lens findings with file:line, per-acceptance-gate pass/fail table, verdict (approve / bounce list with owning task handles).
