# Worker handoff

This prompt eval proves that a Worker records a structured Domios report for
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

The report shows all cases passing for all three samples and the runner
exits zero.

<!-- revision: 3 -->

The lane scenarios require a crate check with four Cargo jobs before committing and one post-commit integration Clippy check; Rust execution tests remain deferred to canonical.
