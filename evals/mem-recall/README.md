# mem-recall bench

Deterministic recall-quality bench for the `mem` module. Seeds a fixture
into a fresh SQLite store, runs every test query through the real search
path, and scores Recall@K / MRR / latency.

## Running

```bash
cd src-tauri
cargo run --bin mem-bench --release -- \
  --fixture ../evals/mem-recall/benchmark.json \
  --output-dir ../evals/mem-recall/results \
  --report-dir ../evals/mem-recall/reports
```

A tempdir under the OS temp is used for the store by default. Pass
`--db PATH` to keep it.

### Compare against a saved baseline

```bash
cargo run --bin mem-bench --release -- \
  --baseline ../evals/mem-recall/baselines/baseline-805aaea1.json
```

### Save the current run as a baseline

```bash
cargo run --bin mem-bench --release -- --save-baseline
```

Writes `baseline-<shortcommit>.json` into `baselines/`.

## Bench mode

The runner builds a `MemManager` via `MemManager::open_offline(...)`.
Compared to normal `open()` this:

- spawns **no** background threads (no embed queue worker)
- calls **no** LLM in default and stub modes (no auto-extraction)
- needs **no** API keys or model weights in default and stub modes
- falls back to FTS5-only recall when sqlite-vec is unavailable

Seeding uses `MemManager::insert_raw(...)` which bypasses the dedup and
auto-extraction pass in `save_inner`. That keeps the store limited to
exactly the items declared in the fixture so recall ranking isn't
polluted by extracted preference / fact children.

## Outputs

- `results/<timestamp>.json` — full per-query results + metrics. Machine
  readable. Carries the git commit for provenance.
- `reports/<timestamp>.html` — self-contained HTML report (inline CSS,
  no external assets). Opens in any browser.
- `baselines/baseline-<commit>.json` — written when `--save-baseline`
  is passed. These are the reference runs the bench diffs against.

## Fixture format

`benchmark.json` is the source of truth:

```json
{
  "description": "...",
  "seed_data": [
    {
      "id": "seed_1",              // stable id used in test expectations
      "kind": "fact",              // any mem_item kind
      "title": "...",
      "body": "...",
      "area": "rekap",             // optional
      "tags": ["..."],             // optional
      "importance": 4.0,           // optional, default 3.0
      "summary": "..."             // optional
    }
  ],
  "test_cases": [
    {
      "id": "T01",                 // stable test id
      "name": "exact_keyword",
      "query": "PgBouncer pooling",
      "expected_top": "seed_2",              // primary: winner must be this
      "expected_top_any": ["seed_2","seed_6"],// or any of these
      "category": "keyword"                  // free-form category tag
    }
  ]
}
```

The fixture ids (`seed_1` etc.) are stable labels — the runner maps
engine-generated ids back to them so the scoring is independent of slug
generation.

## Adding new test cases

1. Add an item to `seed_data` with a unique `id` (e.g. `seed_9`).
2. Add one or more `test_cases` referencing that id in `expected_top`
   or `expected_top_any`.
3. Re-run the bench. New cases flow through automatically.

Keep cases small and targeted — one recall behavior per case (typo,
cross-language, semantic, preference, negation, etc.). The `category`
field is free-form and only used for grouping in reports.

## Metrics

- **Recall@K** — fraction of queries where at least one expected id
  appears in the top K results. Reported at K=1, 5, 10.
- **MRR** — mean reciprocal rank across queries. 0 if no expected hit.
- **p50 / p95 latency** — per-query `recall()` wall time in ms.
- **total wall** — aggregate wall time for all queries.

## CLI wrapper

The `codeterm` CLI (in `cli/`) does **not** yet expose a `mem bench run`
subcommand — the cli crate is HTTP-only and wouldn't gain anything from
wrapping a direct binary invocation. Run the bench via
`cargo run --bin mem-bench` for now. If we ever want it behind
`codeterm mem bench`, the subcommand would just shell out to the built
binary at `target/release/mem-bench`.

## Optional decision re-rank A/B

Run `bash evals/mem-recall/run.sh --decision-rerank stub` for an offline,
deterministic comparison. The bench recalls a pool of at least 10 items for
each positive and answer-absent query, then sends those candidates through the
real stage-2 re-ranker. The stub model scores normalized keyword overlap.
The JSON gains a `decision` block with baseline and reranked Recall@1,
Recall@K, MRR, fallback counts, per-query stage latency, and low-confidence
scores. `decision_negatives.json` supplies answer-absent queries only in stub
and live modes. Low-confidence precision is the fraction of all flagged
queries that are answer-absent; the positive false-positive rate is the
fraction of answer-present queries flagged low-confidence.

`--decision-rerank off` is the default. It preserves the original metrics and
JSON keys. `--check` accepts only off mode and compares only the original
metrics with `baseline.json`.

`bash evals/mem-recall/run.sh --decision-rerank live` uses the stored
TypeSafe token in `<data-dir>/decision-models`. Supply a different root with
`--decision-data-dir PATH`. A missing or unreadable token fails before any
query. **Live mode makes paid API calls, is owner-run only, and must never be
part of an automated gate.**
