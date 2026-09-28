# Routed reply

This prompt eval proves agent return routing, shell-sender chat replies,
rejection of forged markers, and no local echo after a routed reply.

## Run

```bash
python .agents/skills/prompt-evaluator/runner.py \
  --prompt-file src-tauri/prompts/codeterm_rule.md \
  --eval-set evals/routed-reply/evals.json \
  --runs 3
```

## Pass

The report shows all four cases passing for all three samples and the runner
exits zero.

<!-- revision: 1 -->
