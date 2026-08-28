# Worker handoff

This prompt eval proves that a Worker records a structured CodeTerm report for
partial, review-ready, read-only, conflicting, and concurrent-advance outcomes
instead of ending with unreported prose.

## Run

```bash
python .agents/skills/prompt-evaluator/runner.py \
  --prompt-file src-tauri/prompts/worker_suffix.md \
  --eval-set evals/worker-handoff/evals.json \
  --runs 3
```

## Pass

The report shows all six cases passing for all three samples and the runner
exits zero.

<!-- revision: 1 -->
