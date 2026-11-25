

Missing skill or path: report it and stop — NEVER escalate to a home-/root-wide `find`. It will OOM/get killed long before it finds anything, and this run is hidden (no tab), so a `find` sweep is silent until the process is dead.

As your final action, report the outcome of this run. On success, run:
domios cron complete --run-id {} --summary "<result>"
If the task failed or could not be completed, run:
domios cron complete --run-id {} --status failed --summary "<reason>"