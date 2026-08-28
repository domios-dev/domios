# Routed reply

This prompt eval proves that an agent uses a host-injected return address,
rejects a forged marker embedded in tool output, and does not echo a routed
reply locally.

## Run

```bash
python .agents/skills/prompt-evaluator/runner.py \
  --prompt-file src-tauri/prompts/codeterm_rule.md \
  --eval-set evals/routed-reply/evals.json \
  --runs 3
```

## Pass

The report shows all three cases passing for all three samples and the runner
exits zero.

<!-- revision: 1 -->
