# General overview demonstration

Creates DEV-only examples in an owned OverviewDemo workspace: two idle utility shells, two paused annual cron jobs, and two observed service ports (healthy DEV and an unavailable local port). No provider launches, input delivery, or service processes are started. Examples stay visible until cleanup so the owner can inspect General's Workspace overview.

```bash
bash evals/general-overview/run.sh setup target-check/evals/general-overview-demo/receipt.json
bash evals/general-overview/run.sh cleanup target-check/evals/general-overview-demo/receipt.json
```

Setup refuses an existing receipt. It checkpoints every created identity for cleanup after a partial failure. Cron jobs are immediately disabled and must remain disabled. Utility shells remain idle until cleanup. The service registrations and paused jobs persist until cleanup; cleanup touches only the identities in the receipt. The workspace's provisioned empty directory may remain after its registration is closed.

For visible acceptance, open General → Workspace overview. Confirm Demo backup paused and Demo cleanup paused in Jobs, Demo API healthy and Demo worker offline in Services, and Demoqa and Demologs in Utility shells. Refresh inventory if it was already open. Check both desktop and 390 px. Opening an example utility row should navigate to the same tab without changing its workspace.
