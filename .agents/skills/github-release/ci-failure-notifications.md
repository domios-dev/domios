# CI failure notifications

Normal CI uses triage mode: pushes, scheduled fallback runs and PRs leave cancellation off. The CI result aggregate reports named job failures and timeouts. Release reuses the resulting exact-SHA receipts and launches no duplicate test tiers. A failed qualification cannot start release packaging. Reusable diagnostic callers may explicitly opt into the existing first-failure notification/cancellation hook. It does not change signing, qualification or publication requirements.

After deploying the receiver with owner GO, register its destination through the secret store. Pipe a UTF-8 JSON file into the command; never put its token in arguments or a committed file:

```powershell
Get-Content -Raw <private-registration.json> | domios mem secret save --name ci-webhook
```

The private JSON is `{ "token": "<random dedicated token>", "repo": "domios-dev/domios", "plan": "<owning release plan id>" }`. Use `--overwrite` only when intentionally replacing a registration. The stored plan selects its current live, verified Orchestrator; incoming events cannot choose a tab or a plan. Update the registration for a new release plan.

Set repository Actions secrets `DOMIOS_CI_WEBHOOK_URL` to `http://<receiver Tailnet address>:7685/api/ci/webhook` and `DOMIOS_CI_WEBHOOK_TOKEN` to the same dedicated token. Use the approved listener's port if it differs. Runners must reach that Tailnet listener. Keep the token separate from the instance API token and Pebble credential. No plugin registration is needed.

Notification POST has a three-second failure bound; cancellation has five seconds. Neither failure replaces the original test failure. The runner claims one repository/run/attempt/job/runner event under `RUNNER_TEMP`; the receiver also deduplicates that identity through the process-local gateway cache. It does not persist deduplication across a daemon restart. A failed delivery is logged as a warning without replaying payloads.

Rust receiver selector: `inbound_webhook::ci::tests` in `codeterm-api`. With separate owner authorization for a controlled failing workflow, require one notification, one routed owner message naming the exact run/attempt/SHA/step/test, and a cancelled run whose packaging jobs stop. A successful POST alone is not proof of transcript delivery. No live CI failure rehearsal is implicit in source verification.
