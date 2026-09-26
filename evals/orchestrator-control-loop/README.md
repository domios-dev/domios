# Orchestrator control loop

This prompt eval proves that an Orchestrator remains responsive during long
work, batches verification proportionally, preserves the Worker/QA boundary,
waits for acknowledgement rather than treating dispatch readiness as work
started, and leaves a healthy acknowledged Worker autonomous until its report.

## Run

```bash
python .agents/skills/prompt-evaluator/runner.py \
  --prompt-file src-tauri/prompts/orchestrator_suffix.md \
  --eval-set evals/orchestrator-control-loop/evals.json \
  --runs 3
```

## Pass

The markdown report marks all seven cases as passing for all three samples and
the runner exits zero.

<!-- revision: 3 -->
