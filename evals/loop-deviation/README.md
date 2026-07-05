# loop-deviation — anti-stagnation / goal-convergent deviation bench

Verifies the loop engine's per-iteration progress tracking and the
stagnation-triggered, **goal-anchored** deviation directive end-to-end against
the real dev daemon (port 7686).

The engine is deterministic, so this is a deterministic scenario (no model
judging): given the same progress signals it always produces the same
stagnation counter and the same directive string.

## What it asserts

1. **Stagnation → deviation after K.** `K = 2` consecutive no-progress
   `advance` calls. After iter 1 `stagnation == 1` and there is no deviation;
   after iter 2 `stagnation == 2` and a deviation directive appears.
2. **Goal-anchored.** The directive string contains both the loop `target` and
   the plan goal vector — it changes the *angle*, never the goal.
3. **Progress resets.** A `--progress true` advance resets `stagnation` to 0 and
   clears the deviation.
4. **Progressing loop never deviates.** Three progressing iterations keep
   `stagnation == 0` and `deviation == null` throughout.
5. **Budget guard.** A loop at budget refuses to advance further (the
   stagnation logic never lets exploration run past the iter/budget bound).

## Run

The daemon must be built from this branch (the bench needs the
`plan.loop.advance` op + the deviation engine). Then:

```bash
./build.sh dev --wait          # dev on 7686, built from lane/loop-state
evals/loop-deviation/run.sh
```

The script sources `evals/_lib/lib.sh` and uses `dc` / `dev_alive`, so it is
locked to dev (7686) and fails with exit 7 if dev is down — it can never touch
prod (7685). It creates a throwaway plan + loops and deletes the plan on exit
(`trap cleanup EXIT`).

Expected tail: `== loop-deviation eval passed ==`.

## Manual live smoke (subset)

```bash
source evals/_lib/lib.sh
PID=$(jq -r .plan_id <<<"$(dc --json plan create --title smoke)")
dc plan goal set "$PID" --vector "ship a self-correcting loop engine"
LID=$(dc plan loop create "$PID" --target "make it converge" --budget 5 --stop-when done)
dc plan loop advance "$LID" --progress false
dc plan loop advance "$LID" --progress false   # deviation now set
dc --json plan loop status "$LID" | jq '{count,stagnation,deviation}'
dc plan delete "$PID"
```
