# Domios test harness references

Moved from the embedded TEST-AUTHORING charter. These are repository details,
not instructions for production agents in other projects.

Playwright remains suspended under the testing skill. The references below
record the archived harness contracts; they do not authorize running it.

## 4. Playwright mechanics

The Playwright suite has concrete helper contracts; use them instead of
re-deriving timing and cleanup in each spec.

- Canvas and Mission Control specs wait on `waitForCanvasSettled` from
  `tests/playwright/helpers/canvas-inspect.ts`. It watches projected geometry
  across animation frames, so a spec waits for the app surface instead of a
  blanket sleep.
- Daemon-killing coverage is isolated in the restart project. Specs tagged
  `@restart` are excluded from shared `smoke`, `golden`, and `full` runs, then
  executed serially so ordinary shards do not lose their daemon mid-test.
- The default fixture snapshots state with `snapshotBaseline` and tears down
  only test-created work with `diffReset` from `tests/playwright/helpers/reset.ts`.
  Fixture setup may self-heal a dead dev daemon, but one crash must fail one
  test, not cascade into a whole shard.
- Agent-flow golden tests use the debug-only `CODETERM_STUB_AGENT_CMD` transport
  when real provider liveness is not the oracle. The replayer lives under
  `tests/playwright/fixtures/stub-agent/` and preserves the real provider launch,
  parser, detection, status, and session paths.
- Playwright measurement harnesses live outside flow assertions. Latency, rAF,
  longtask, and replay-speed probes produce metrics artifacts; product-flow
  specs assert behavior and keep their own caps tight.

check-queue, check-tests and clippy --all-targets type-check the bin against MockRuntime.
The Wry world is checked by clippy --workspace in the full gate, by check-queue's dev-lib check (Tone, task_10jzuo) and by every dev and production build.

