# Live memory decision re-rank evaluation

This evaluation compares the `supporter off` baseline with decision
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
and runner from a Team-bound utility shell. They set `CODETERM_INSTANCE` only in each child shell that
sources the dev-pinned `evals/_lib/lib.sh`; all dev requests go through `dc`.
Runners save private results and print their paths; the Orchestrator reports them outside the bench. Runners never invoke the production CLI or send messages.

```bash
python3 evals/mem-rerank-live/export.py
# Review or regenerate the private questions.json against the new snapshot.
bash evals/mem-rerank-live/import.sh memrerank
python3 evals/mem-rerank-live/run.py --dev-instance memrerank
# Include docs and past-session candidates in the applied arm.
python3 evals/mem-rerank-live/run.py --dev-instance memrerank --docs 3 --sessions 2
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
current `memrerank` instance uses port 7695). The importer checks dev node IDs
through `dc` before saving. It adds a stable source-derived title suffix when a
snapshot item would collide with another snapshot item or an existing dev item,
records the ID returned by `mem save --json`, and keeps only snapshot items in
the private source-to-dev map. Existing dev seeds remain outside that map.

Run the pure metric tests offline with:

```bash
python3 -m unittest evals/mem-rerank-live/test_metrics.py
```

## Questions and scoring

Each private question has `id`, `category`, `query`, `answer_ids`, `kind`, and
`note`; optional `session_answer_ids` lists provider session ids when a past
session is a valid answer. `answer_ids` contain original snapshot IDs. `run.py`
maps memory IDs through the private import map and compares session IDs as
returned; titles never determine relevance. `answerable` and `decoy_buried` questions contribute to R@1, R@3,
and MRR@10. `absent` and `decoy_absent` questions contribute to gate
precision/recall. A missing maximum relevance is not treated as a gate.

The rerank arm makes live TypeSafe requests. Per-candidate mode costs N requests per search; shared-state mode sends N named candidate questions in one request. The global default cap is 600 requests/hour, shared with tab observations. An 80-question shared-state run uses about 80 rerank requests; token usage still scales with the state/questions. Do not run live calls as an automated gate.

For qualification, preserve the pre-Spine build's results first, then run the multi-question build with `--request-mode shared_state`. The retained `--request-mode per_candidate` supports a same-build comparison and both flags restore the prior selection. R@1 and MRR@10 must not regress beyond about 0.035; report p50/p95 and requests/search as well. The shipping default remains per-candidate until canonical A/B qualifies promotion. The runner saves before/after status counters and their per-search delta; isolate ambient observations, avoid rolling-day expiry during the run, and use adapter-side request telemetry for older baseline builds without counters.

```bash
# Pre-Spine build (no request-mode selector):
python3 evals/mem-rerank-live/run.py --dev-instance memrerank --docs 3
# Multi-question build:
python3 evals/mem-rerank-live/run.py --dev-instance memrerank --docs 3 --request-mode shared_state
```

`--docs M` and `--sessions M` set their matching source budgets on the named
dev instance for the evaluation, record returned source hits and metrics, and
restore the previous values even when a query fails. The limits are `0..=10`
for docs and `0..=5` for sessions; omitted leaves that config value untouched.

## CLI usage-error probes

`cli_mistakes.json` is a separate, public fixture of 15 CLI mistakes sourced
from the `pl_blazzd` task history and the CodeTerm skill's known wrong-flag
list. It contains synthetic probe identifiers only. Its `error_text` values
were captured from the named `memrerank` dev CLI through `dc`.

Run it from the parent Orchestrator environment after rebuilding the named dev
instance with the CLI-reference corpus:

```bash
python3 evals/mem-rerank-live/cli_eval.py --dev-instance memrerank
```

The runner requires `supporter.memory_rerank.sources.docs >= 3` and the shipping
`sessions = 0` default. It only reads those settings, then sends one
`--supporter on` query per fixture through `dc`; it does not change config.
Results stay under the private `mem-eval-local/results/cli-mistakes/` directory,
and the runner prints the private summary path for the Orchestrator. The fixture's expected
values are CLI heading titles such as `codeterm plan task add`: the compact
response's `pointer` field carries that title with a trailing ` --help` suffix.

The summary reports hit@1 and hit@3, the median top-1 relevance for correct and
wrong results, confident-but-wrong counts at 0.50/0.70/0.85, and p50 latency.
The preselected gate uses hit@1 >= 70% and confident-but-wrong at 0.70 <= 1/15.
It prints whether to proceed with or cancel the CLI error hint task.
