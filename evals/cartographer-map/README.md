# Cartographer map

This prompt eval proves that the Cartographer generates the essence map from
the source essences rather than drawing it, walks the full coverage checklist,
reports gaps and budget overflow instead of inventing or trimming structure,
and tells the owner once at a release rather than per map change.

The prompt under test is the composed contract the role receives: the Worker
suffix plus the map-authoring checklist. Compose it before running, so the eval
measures what the agent actually gets:

```bash
cat src-tauri/prompts/worker_suffix.md src-tauri/prompts/cartographer_suffix.md \
  > "$TMPDIR/cartographer_contract.md"
```

## Run

```bash
python .agents/skills/prompt-evaluator/runner.py \
  --prompt-file "$TMPDIR/cartographer_contract.md" \
  --eval-set evals/cartographer-map/evals.json \
  --runs 3
```

For an A/B against the previous revision of the checklist, run the same eval set
with `--prompt-a` and `--prompt-b` over two composed files.

## Pass

The report shows all eight cases passing for all three samples and the runner
exits zero.

<!-- revision: 1 -->
