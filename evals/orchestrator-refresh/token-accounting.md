# Refresh token accounting — cycle cost vs drift cost

The refresh cycle spends tokens (handoff snapshot + re-orient injection + the
orchestrator's re-read of plan state). The thing it PREVENTS is drift: an
orchestrator running on stale in-head context re-deriving the frontier wrong
over many turns. Record real numbers here after a drill so the trade is honest.

## How to read the cost

The driver logs the wall-clock cost per cycle:

```
grep 'cycle_cost_wall_ms' ~/.codeterm-dev/logs/codeterm.log | tail -1
```

Token cost is read from the provider's own usage counters around the cycle.
Capture usage before the trigger and after `resumed`:

```
dc tab list --json | jq '.tabs[] | select(.id=="<TAB>") | .usage'   # before + after
```

## The comparison to record

| Metric                        | Value | Source |
|-------------------------------|-------|--------|
| Refresh cycle — tokens        |  TBD  | usage delta trigger→resumed |
| Refresh cycle — wall ms       |  TBD  | `cycle_cost_wall_ms` log line |
| 10 post-drift turns — tokens  |  TBD  | usage delta over 10 turns with NO refresh, on stale context |
| Drift errors avoided          |  TBD  | # of wrong-frontier actions in the no-refresh arm |

Fill both arms from the same plan/orchestrator so the context size is
comparable. The claim to validate: one refresh cycle costs less than the
compounding waste of 10 drifted turns.
