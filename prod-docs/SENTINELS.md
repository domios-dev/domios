# Sentinels

A sentinel watches external status cheaply while an agent does useful work.
It runs a bounded shell probe at the requested cadence, hashes the captured
result, and classifies only changed output. Use it to wait for CI or build
completion instead of repeatedly asking an agent to run the same status check.

## Create a watch

```bash
codeterm sentinel create --probe "gh run view 456 --json status,conclusion" --question "waiting while CI is in progress; finished when CI has completed with any conclusion" --buckets waiting,finished --every 20s --until finished --timeout 1h
```

The first probe runs immediately. `--every` defaults to `20s` and must be at
least `10s`. Durations accept `s`, `m`, `h`, `d`, and combinations such as
`1h30m`; a bare number means minutes. The maximum duration is 366 days.
`--timeout` bounds the lifetime. `--until` names one declared bucket and ends
the watch after that bucket's first routed transition. `--route bucket=stop`
ends it without sending a message.

The creator's live tab supplies its cwd, workspace, and Team. Commands are
executed as the daemon user in that cwd, with no interactive stdin. Each probe
has a three-second runtime limit (ten seconds on Windows, where PowerShell starts slowly), or the remaining lifetime if shorter.
Capture retains up to 32 KiB each of stdout and stderr. The exit status is part
of the hashed result. A probe timeout or spawn failure records a skip.

## Classification and transitions

A connected Supporter adapter with Choice capability gets one question over
`--buckets` and the automatic `other` option, using the shared request budget
and journal. Probe output is treated as untrusted data. Unchanged output never
calls the model, including after a failed or budget-exhausted classification.

Without a connected Choice adapter, supply deterministic patterns:

```bash
codeterm sentinel create --probe "check-ci" --question "CI result" --buckets waiting,passed,failed --match 'passed=(?m)^success$' --match 'failed=(?m)^failure$' --every 20s --until passed --timeout 1h
```

Patterns see the formatted result: exit status, stdout, then stderr. Match the
status line with a multiline pattern such as `(?m)^success$` when the probe
prints `success`. Exactly one matching bucket is accepted at confidence 1;
zero or ambiguous matches become `other`. Create without either a connected
Choice adapter or a pattern is refused.

Routes run once when a new accepted bucket differs from the previous accepted
bucket, including the first accepted result. Model confidence must be at least
0.80. `other`, low confidence, adapter failure, and exhausted budgets take no
route and are journaled. Unknown or weak observations do not reset the last
accepted bucket. Returning to a previously accepted bucket after another
accepted bucket is a new transition.

## Closed-set routes

Repeat `--route bucket=action` to assign routes. Unassigned buckets use `notify`.

| Action | Result |
|---|---|
| `notify` | Sends the result to the creator agent; a shell creator gets a notification record |
| `send:<tab>` | Sends a result-file pointer through the existing tab message API |
| `cron:<name>` | Triggers the uniquely named existing cron |
| `stop` | Stops the sentinel |

A send target must be an agent in the creator's Team and workspace, so probe
text cannot become shell input. Result bodies live in the recipient's AgentPad.
Cron targets must belong to the same Team and workspace. Targets resolve at
creation and are checked again at routing. No route executes a command chosen
by the model. Transition claims are persisted before routing; a failed route is
journaled and is not retried for the same accepted bucket.

## Inspect and stop

```bash
codeterm sentinel list --json
codeterm sentinel show SENTINEL_ID --json
codeterm sentinel stop SENTINEL_ID --json
```

These commands show or manage only the calling tab's sentinels in its current
Team and workspace. `show` includes the probe, accepted bucket, hash, next run,
expiry, and last decision reason. Stopped watches remain inspectable.
Closing a Team stops and removes all its sentinels, including in-flight route
claims. A removed creator or changed Team stops the watch. The store admits
at most 64 active and 256 total sentinels. Watches are local to the creator's
machine; `--mesh` is refused.

<!-- revision: 2 -->
