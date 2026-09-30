# Live memory decision re-rank evaluation

This evaluation compares the `decision-rerank off` baseline with decision
re-rank on a private copy of production memory. It never stores personal snapshot,
question, import-map, or result data in the repository.

## Private data

All private files live under:

```text
~/Developer/codeterm-worktrees/mem-eval-local/
```

The exporter writes `snapshot/items.jsonl` and `snapshot/graph.json`. The
curated question set is `questions.json`; the importer writes `id-map.json`;
each run writes `results/<UTC timestamp>/results.json` and `summary.md`.
Keep this directory local and out of git. The export uses the parent production
CLI connection on port 7685 with `CODETERM_INSTANCE` removed. It enumerates
items and graph nodes through CLI read commands, skips archived items, and
refuses to write the snapshot inside the repository. `mem get` updates read
access telemetry on production; it does not change item content.

## Rerun

Run the exporter from the parent production tab environment. Run the importer
and runner from a Team-bound utility shell whose `CODETERM_PARENT_TAB_ID` is the
Orchestrator tab. They set `CODETERM_INSTANCE` only in each child shell that
sources the dev-pinned `evals/_lib/lib.sh`; all dev requests go through `dc`.
The runner checks the parent connection before making any dev or live-model
calls and sends its summary through that parent connection.

```bash
python3 evals/mem-rerank-live/export.py
# Review or regenerate the private questions.json against the new snapshot.
bash evals/mem-rerank-live/import.sh memrerank
python3 evals/mem-rerank-live/run.py --dev-instance memrerank
```

Keep every question and result local. Check each positive `answer_ids` entry
against the snapshot. For absent probes, search both exported files before
keeping the question, for example:

```bash
rg -i -F 'distinctive candidate fact' \
  ~/Developer/codeterm-worktrees/mem-eval-local/snapshot/items.jsonl \
  ~/Developer/codeterm-worktrees/mem-eval-local/snapshot/graph.json
```

An absent probe is supported when that search has no matches; check plausible
synonyms and identifiers as well.

The exporter reads the production instance; it does not save or update memory
items. The import and run steps target only the named dev instance through
`dc`. The dev instance must be available at its registered connection (the
current `memrerank` instance uses port 7695).

Run the pure metric tests offline with:

```bash
python3 -m unittest evals/mem-rerank-live/test_metrics.py
```

## Questions and scoring

Each private question has `id`, `category`, `query`, `answer_ids`, `kind`, and
`note`. `answer_ids` always contain original snapshot IDs. `run.py` maps those
IDs through the private import map before scoring; titles never determine
relevance. `answerable` and `decoy_buried` questions contribute to R@1, R@3,
and MRR@10. `absent` and `decoy_absent` questions contribute to gate
precision/recall. A missing maximum relevance is not treated as a gate.

The rerank arm makes live TypeSafe calls. Budget roughly 10 calls per query;
an 80-question run can therefore make about 800 billable calls. Do not run it
as an automated gate.
