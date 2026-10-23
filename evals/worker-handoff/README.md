# Worker handoff

This prompt eval proves that a Worker records a structured Domios report for
partial, review-ready, read-only, conflicting, and concurrent-advance outcomes,
and batches verification without inventing project-specific commands.

## Run

```bash
python .agents/skills/prompt-evaluator/runner.py \
  --prompt-file src-tauri/prompts/worker_suffix.md \
  --eval-set evals/worker-handoff/evals.json \
  --runs 3
```

## Pass

The report shows all cases passing for all three samples and the runner
exits zero.

<!-- revision: 5 -->

The lane scenarios supply their repository's check commands as fixture context; these commands are not universal Worker instructions. Cases 10–11 cover coherent edit batches and rerunning only affected checks; case 12 covers ending on one short line after the report.
