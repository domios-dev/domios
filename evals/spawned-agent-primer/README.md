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

For the sentinel change, preserve the pre-edit arm with baseline ref
`776d003d1b65a5edd422270b771d0a1d3d59ee7a`. In canonical after merge:

```bash
python3 evals/general-agent/prepare-prompts.py \
  --baseline-ref 776d003d1b65a5edd422270b771d0a1d3d59ee7a \
  --output-dir /tmp/lookout-primer-bench
TEST_CAP_SECS=3600 scripts/test-capped.sh --exec \
  python3 .agents/skills/prompt-evaluator/runner.py \
  --prompt-a /tmp/lookout-primer-bench/spawned-agent-old.md \
  --prompt-b /tmp/lookout-primer-bench/spawned-agent-new.md \
  --eval-set evals/spawned-agent-primer/evals.json --runs 3 --report markdown
```

Case 9 asserts the `sentinel create` mechanism, cadence/end flags, and absence
of an agent polling loop; case 11 covers the `cron-run` observation source and
its recorded run tab. Neither case asserts prompt wording. The old arm records
the baseline and the new arm must pass 3/3 without regression in cases 1–10.
Record the observed A/B result as a plan stride.

## Pass

The new-arm report shows all twelve cases passing for all three samples and
the runner exits zero. The old arm may fail the new CronRun case; it records
the baseline while cases 1–10 guard against regressions.

Case 12 covers utility purposes, creation descriptions, idle restart and closing a completed tab. The prepared non-Worker prompt includes `utility_tab_guidance.md`; the Worker composition is covered by the Rust fragment-delivery test.

<!-- revision: 8 -->
