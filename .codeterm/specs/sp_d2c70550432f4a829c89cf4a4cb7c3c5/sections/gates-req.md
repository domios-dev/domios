Acceptance gates for the whole effort (Orchestrator judges done against these):

1. Functional: default view is Board; switch order Board/List/Canvas; mode persists across close/reopen; task detail opens from Board card, List row, and Canvas card; Esc restores prior state in each.
2. Done-heavy: with a fixture plan of 200 done + 10 active tasks, Board page height stays bounded, List defaults collapsed for done-only sections, Canvas clusters done tasks; verified via perfFixture/Playwright fixture.
3. Perf: Canvas pan/zoom on the 200-task fixture has no dot-cloud state and sustains interactive frame rate (frame-probe evidence in dev log or test output).
4. Visual: fresh screenshots of Board, List, Canvas (normal + done-heavy fixture) are captured via Playwright web-mode and attached to the task reports for user judgment. The user (via Orchestrator) is the final visual acceptance authority — "pleasant" is judged by a human, not a pixel diff.
5. Regression: npm run test:unit green; npx playwright test --project=golden green on macOS web-mode including suites/13-canvas/plan-view-mode.spec.ts updated for the new order/default.
6. Code health: no file exceeds 1000 lines (scripts/check-file-sizes.sh), no new lint errors (npm run lint), Tailwind + var(--ct-*) theme tokens only, testids preserved/extended for e2e.
