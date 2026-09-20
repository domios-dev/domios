# Spawned-agent primer

This prompt eval proves that every spawned agent knows the shared rendering
surface, its sandbox limits, tab identity, batched-edit discipline, role-scoped
notification and reporting rules, and that a question for the user goes through
notify rather than a provider-native ask tool.

## Run

```bash
python .agents/skills/prompt-evaluator/runner.py \
  --prompt-file src-tauri/prompts/agent_codeterm_primer.md \
  --eval-set evals/spawned-agent-primer/evals.json \
  --runs 3
```

## Pass

The report shows all seven cases passing for all three samples and the runner
exits zero.

<!-- revision: 3 -->
