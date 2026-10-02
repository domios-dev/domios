# Sentinels for external status

Use `codeterm sentinel create --help` and `codeterm docs SENTINELS` when waiting
for CI, a build, or another external status. Do useful work while the scheduled
probe runs; avoid agent polling loops. Sentinels inherit the creator's cwd,
workspace, and Team, and Team close removes them.

```bash
codeterm sentinel create --probe "gh run view 456 --json status,conclusion" --question "waiting until CI completes, then finished regardless of conclusion" --buckets waiting,finished --every 20s --until finished --timeout 1h
codeterm sentinel list --json
codeterm sentinel show SENTINEL_ID --json
codeterm sentinel stop SENTINEL_ID --json
```

Cadence has a 10-second minimum. Each probe runs for at most three seconds and
captures bounded output. Unchanged output never calls the Supporter. Changed
output uses one Choice question, automatic `other`, and the shared budget;
without a connected Choice adapter, add `--match bucket=regex`.

Only a new accepted bucket at confidence >= 0.80, or an unambiguous regex match,
routes. Routes are `notify` (to the creator, default), `send:<tab>` (a same-Team
agent, body via file), `cron:<name>` (an existing same-Team cron), and `stop`.
Repeat `--route bucket=action` to configure buckets. `--until bucket` ends the
watch after its first routed transition, and `--timeout duration` bounds its
lifetime. Model verdicts never become arbitrary commands.

<!-- revision: 1 -->
