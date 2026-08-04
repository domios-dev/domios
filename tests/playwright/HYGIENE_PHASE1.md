# Phase 1 hygiene report

## Header gate red-green

Red demo used a temporary scoped spec with no header:

```text
RED exit=1
Playwright spec header violations:
tests/playwright/suites/zz-header-gate-demo.spec.ts: missing spec header
```

Green demo added the required header to the same temporary scoped spec:

```text
GREEN exit=0
Playwright spec headers OK
```

Final full-suite static result:

```text
Playwright spec headers OK
```

## Wait sweep

`HEAD` contained 66 `waitForTimeout` calls under `tests/playwright/suites`.
After this pass there are 66 remaining calls, all with an adjacent
`PW_JUSTIFIED_SLEEP:` reason, so unmarked hard-sleep debt is 0. The stale
baseline entries for removed/unmarked sleeps were deleted; future unmarked
`waitForTimeout` calls fail `pnpm test:pw:lint`.

| Spec | Before waits | After waits | Justified |
| --- | ---: | ---: | ---: |
| `tests/playwright/suites/12-web/tab-switch-bounce.spec.ts` | 1 | 1 | 1 |
| `tests/playwright/suites/13-canvas/_diag.spec.ts` | 1 | 1 | 1 |
| `tests/playwright/suites/13-canvas/canvas-calm-ux.spec.ts` | 3 | 3 | 3 |
| `tests/playwright/suites/13-canvas/canvas-nav.spec.ts` | 3 | 3 | 3 |
| `tests/playwright/suites/13-canvas/canvas-perf.spec.ts` | 4 | 4 | 4 |
| `tests/playwright/suites/13-canvas/canvas-plan-document.spec.ts` | 5 | 5 | 5 |
| `tests/playwright/suites/13-canvas/canvas-presence.spec.ts` | 3 | 3 | 3 |
| `tests/playwright/suites/13-canvas/canvas-reconnecting-banner.spec.ts` | 2 | 2 | 2 |
| `tests/playwright/suites/13-canvas/canvas-section-agent.spec.ts` | 2 | 2 | 2 |
| `tests/playwright/suites/13-canvas/canvas-section-zones.spec.ts` | 3 | 3 | 3 |
| `tests/playwright/suites/13-canvas/canvas-spec-plan-link.spec.ts` | 3 | 3 | 3 |
| `tests/playwright/suites/13-canvas/canvas-task-titles.spec.ts` | 2 | 2 | 2 |
| `tests/playwright/suites/13-canvas/canvas-ux1-schematic.spec.ts` | 2 | 2 | 2 |
| `tests/playwright/suites/13-canvas/canvas-visual-sweep.spec.ts` | 1 | 1 | 1 |
| `tests/playwright/suites/13-canvas/canvas-working-points.spec.ts` | 1 | 1 | 1 |
| `tests/playwright/suites/13-canvas/canvas-worlds-v2.spec.ts` | 1 | 1 | 1 |
| `tests/playwright/suites/13-canvas/canvas-worlds-v3.spec.ts` | 1 | 1 | 1 |
| `tests/playwright/suites/13-canvas/canvas-zoom-drift.spec.ts` | 1 | 1 | 1 |
| `tests/playwright/suites/13-canvas/mission-control-flow.spec.ts` | 1 | 1 | 1 |
| `tests/playwright/suites/13-canvas/plan-header-mobile.spec.ts` | 1 | 1 | 1 |
| `tests/playwright/suites/13-canvas/tab-switch-latency.spec.ts` | 7 | 7 | 7 |
| `tests/playwright/suites/16-chat-view/composer-focus-steal.spec.ts` | 1 | 1 | 1 |
| `tests/playwright/suites/16-chat-view/cursor-new-rebinds-session.spec.ts` | 2 | 2 | 2 |
| `tests/playwright/suites/16-chat-view/opencode-assistant-replies.spec.ts` | 1 | 1 | 1 |
| `tests/playwright/suites/16-chat-view/send-on-fresh-tab.spec.ts` | 1 | 1 | 1 |
| `tests/playwright/suites/18-reorder/pin-general.spec.ts` | 8 | 8 | 8 |
| `tests/playwright/suites/18-reorder/reorder.spec.ts` | 5 | 5 | 5 |

## Weak oracles

See `tests/playwright/WEAK_ORACLES.md`.

Sole weak final assertions before sweep: 7. Strengthened in-place: 6.
Justified residue: 1 helper-wrapped runtime-error oracle.
