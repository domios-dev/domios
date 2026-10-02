# Spawned-agent primer

This prompt eval proves that every spawned agent knows the shared rendering
surface, its sandbox limits, tab identity, batched-edit discipline, role-scoped
notification and reporting rules, and that a question for the user goes through
notify rather than a provider-native ask tool.

## Run

```bash
python3 evals/general-agent/prepare-prompts.py \
  --baseline-ref HEAD --output-dir /tmp/spawned-agent-prompt-bench
TEST_CAP_SECS=3600 scripts/test-capped.sh --exec \
  python3 .agents/skills/prompt-evaluator/runner.py \
  --prompt-file /tmp/spawned-agent-prompt-bench/spawned-agent-new.md \
  --eval-set evals/spawned-agent-primer/evals.json --runs 3
```

The Guide directive case needs the composed primer plus rendering guidance;
use the preparation and A/B commands in `evals/general-agent/README.md` when
changing that fragment. Evaluating the primer alone omits the rendering contract.

## Pass

The report shows all eight cases passing for all three samples and the runner
exits zero.

<!-- revision: 4 -->
