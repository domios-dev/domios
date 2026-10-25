# CI failure notifications

The release caller sets `cancel_run: true` on both test tiers. Ordinary PR/push callers leave it false. The first observed failure posts to Domios, then invokes `gh run cancel` for the current `GITHUB_RUN_ID` and repository. This cancels packaging and sibling tests in the same run. It does not change signing, qualification or publication requirements.

After deploying the receiver with owner GO, register its destination through the secret store. Pipe a UTF-8 JSON file into the command; never put its token in arguments or a committed file:

```powershell
Get-Content -Raw <private-registration.json> | domios mem secret save --name ci-webhook
```

The private JSON is `{ "token": "<random dedicated token>", "repo": "rollacode/codeterm", "plan": "<owning release plan id>" }`. Use `--overwrite` only when intentionally replacing a registration. The stored plan selects its current live, verified Orchestrator; incoming events cannot choose a tab or a plan. Update the registration for a new release plan.

Set repository Actions secrets `DOMIOS_CI_WEBHOOK_URL` to `http://<receiver Tailnet address>:7685/api/ci/webhook` and `DOMIOS_CI_WEBHOOK_TOKEN` to the same dedicated token. Use the approved listener's port if it differs. Runners must reach that Tailnet listener. Keep the token separate from the instance API token and Pebble credential. No plugin registration is needed.

Notification POST has a three-second failure bound; cancellation has five seconds. Neither failure replaces the original test failure. The runner claims one repository/run/attempt/job/runner event under `RUNNER_TEMP`; the receiver also deduplicates that identity through the process-local gateway cache. It does not persist deduplication across a daemon restart. A failed delivery is logged as a warning without replaying payloads.

Focused local proof: `node --test scripts/tests/ci-first-failure.test.mjs scripts/tests/ci-rust-tests.test.mjs scripts/tests/release-test-gate.test.mjs scripts/tests/test-catalogue-ci.test.mjs`, plus `node scripts/test-catalogue.mjs validate`; Rust receiver selector: `inbound_webhook::ci::tests` in `codeterm-api`. With separate owner authorization for a controlled failing workflow, require one notification, one routed owner message naming the exact run/attempt/SHA/step/test, and a cancelled run whose packaging jobs stop. A successful POST alone is not proof of transcript delivery. No live CI failure rehearsal is implicit in source verification.
