# Orchestrator control loop

This prompt eval samples whether an Orchestrator remains responsive during long
work, batches verification proportionally, preserves the Worker/QA boundary,
waits for acknowledgement rather than treating dispatch readiness as work
started, and leaves a healthy acknowledged Worker autonomous until its report.
It also checks explicit task acceptance and status readback, separation from
final-release verification, and concrete action on reported work and blockers.

## Run

The command below tests the suffix fragment. For launch-composition evidence,
render the substitutions and appended manual from
`src-tauri/src/prompts.rs::orchestrator_suffix` and pass that retained file.
Compare both variants using the same composition and fixtures. The runner does
not expand every role-specific placeholder automatically.

```bash
python .agents/skills/prompt-evaluator/runner.py \
  --prompt-file src-tauri/prompts/orchestrator_suffix.md \
  --eval-set evals/orchestrator-control-loop/evals.json \
  --runs 3
```

## Pass

The markdown report marks all nine cases as passing for all three samples and
the runner exits zero.

These are model decision samples, not execution of the proposed commands or
proof of live task transitions. Read the sample and judge result together;
invented commands or missing transitions remain failures. In particular, a
judge may confuse reading task records with inspecting a healthy Worker, or
accept invented recovery flags. Check commands against the CLI contract.

The October 3 focused composed A/B (one fresh Haiku sample per case) improved
case 8 from fail to pass and retained case 6 as passing. Case 9 failed in both
variants, including invented recovery/update flags in the candidate. This is
scoped closure evidence, not a nine-case pass or proof of live recovery.

<!-- revision: 4 -->
