# task_bvyp7r evidence

Branch: qa/task_bvyp7r-98241fb1
Base: 98241fb1679da0f6fdcd441de7d5942ba589e977
Instance: task_bvyp7r
Assigned API port: 7815
Derived Vite port: 1519
Derived CDP port: 9322

## Exact reproduction command

```bash
flock /tmp/pl_as7ty5-linux-reds.lock bash -lc 'export PATH=/home/rollacode/.local/node/bin:$PATH; export CODETERM_INSTANCE=task_bvyp7r CODETERM_API_PORT=7815 CT_E2E_PORT=7815 CT_E2E_DATADIR=.codeterm-dev-task-bvyp7r CT_E2E_WEB_MODE=1 VITE_CODETERM_TEST=1 CODETERM_TEST_CLOCK=1; pnpm exec playwright test --config=tests/playwright/playwright.config.ts --project=golden tests/playwright/suites/12-web/tab-switch-bounce.spec.ts -g "@golden web tab-switch does not bounce to previous tab" --timeout=30000 --global-timeout=600000 --reporter=line'
```

## Result

The exact Playwright title did not reach the product test body.

Global setup attempted `bash build.sh dev --wait`, and `build.sh` rejected the QA branch because this checkout is not the registered stability/integration checkout:

```text
ERROR: no release/*-integration worktree is registered for dev.
ERROR: no release/*-integration worktree is registered for dev.
Error: Command failed: bash build.sh dev --wait
```

This is a harness lifecycle failure, not a product assertion failure.

## Follow-up attempt

I queued a lane-scoped manual prep/start path under the required lock to avoid touching shared dev/prod:

```bash
flock /tmp/pl_as7ty5-linux-reds.lock bash -lc 'set -euo pipefail; export PATH=/home/rollacode/.local/node/bin:$PATH; export CODETERM_INSTANCE=task_bvyp7r CODETERM_API_PORT=7815 CODETERM_VITE_PORT=1519 CODETERM_CDP_PORT=9322 CODETERM_DEV_IDENTIFIER=com.codeterm.dev.task.bvyp7r CODETERM_DEV=1 VITE_CODETERM_TEST=1 CODETERM_TEST_CLOCK=1; pnpm exec vite build; node scripts/prepare-cli-stub.js; node scripts/prepare-embed-stub.js; node -e "...generate src-tauri/tauri.dev.conf-task-bvyp7r.json..."'
```

It never acquired the lock. After more than eight minutes, the lock owner was still another lane:

```text
flock /tmp/pl_as7ty5-linux-reds.lock ... CODETERM_INSTANCE=task_4g18d9 CODETERM_API_PORT=7811 ...
```

I stopped only my queued process with Ctrl-C so this lane would not start unexpectedly later.

## Static product read

The current source already has a pending tab switch guard:

- `src/store/multiplexer/index.tsx` installs `pendingTabSwitchRef.current[workspaceId] = { tabId, deadline: Date.now() + 1000 }` before `mux_set_active_tab_and_workspace`.
- `src/store/multiplexer/pendingTabGuard.ts` masks stale backend snapshots back to the guarded tab while the target tab exists and the deadline is live.

No product edit was made because the exact product failure was not reproduced.
