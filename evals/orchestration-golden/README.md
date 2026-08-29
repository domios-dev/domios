# Orchestration golden

This wrapper runs the creation matrix, restore matrix, refresh drill, and plan
report round trip as one orchestration regression tier.

## Run

Start an exact-head dev daemon on port 7686, then run:

```bash
bash evals/orchestration-golden/run.sh
```

## Pass

Its summary reports `PASS=4  FAIL=0`; every named harness has `PASS` and the
command exits zero. Logs are retained under `target-check/evals/orchestration-golden/`.

<!-- revision: 1 -->
