# PREREG: one global local memory embedder

This file is committed before the first candidate recall, throughput or oracle measurement. No b4 query has been read. Candidate order and the rules below are fixed before results. All runs use CPU Candle BertModel, 384 dimensions, a temporary store and one active global provider. Nothing is pushed. The delivery branch contains no Blueprint recall commits.

## Descriptors

| Label | Hugging Face model | Revision | Pooling | Query prefix | Passage prefix | Window |
|---|---|---|---|---|---|---|
| l12 | sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2 | main (cached e8f8c211226b894fcb81acc59f3b34ba3efd5f42) | mean | empty | empty | 512 |
| multilingual-e5-small | intfloat/multilingual-e5-small | 614241f622f53c4eeff9890bdc4f31cfecc418b3 | mean | `query: ` | `passage: ` | 512 |
| bge-small-en-v1.5 | BAAI/bge-small-en-v1.5 | 5c38ec7c405ec4b44b94cc5a9bb96e735b38267a | CLS | `Represent this sentence for searching relevant passages: ` | empty | 512 |
| e5-small-v2 | intfloat/e5-small-v2 | ffb93f3bd4047442299a41ebb6fa998a38507c52 | mean | `query: ` | `passage: ` | 512 |

Chair decision before measurement: the task's assumed L12 window of 128 is superseded by 512, the accepted provider behavior and B1's documented window. No 128 sensitivity arm. Candidates are immutable revisions; L12 retains its accepted descriptor and uses the recorded cached revision offline. Prefixes are applied after the existing global preprocessing. Pooling masks padding and normalizes to unit length.

The multilingual checkpoint's config declares `BertModel`, `model_type=bert`, 384 hidden dimensions and 512 absolute positions; its tokenizer class is XLMRobertaTokenizer. Do not infer a RoBERTa position offset from the tokenizer name. Compare production Candle output with Transformers AutoModel reference vectors before accepting compatibility. Oracle tolerance: cosine >= 0.999 for each passage and query case, including English, Russian, Unicode and truncation. Size means total bytes of the five required model artifacts (dereference cache links). Throughput means warm production passage/query embeds per second over fixed public cases, plus whole-snapshot re-embedding wall time and items/sec.

## Inputs and frozen configuration

Accepted provider base: 12ad2c521628c24e8da9b8830c8f2213e552590d. B1 harness: feat/blueprint-recall at 5894371506a83cb13d787c6bb03ebbce3ed17901. B1 PREREG: 88f2e63b042dbe91b3b7cbba99bd849b636ab766. Frozen config: C10, committed at e5f666bb37da53c9e5639d9d72901ad68bc1cdaf. Config, corpus, cached agent intents and cached enrichment are unchanged.

Real-merge B1 into local branch `eval/embedder-on-recall` after this preregistration. That branch is never pushed or delivered. Provider API compatibility adaptations on scratch only may replace removed model-id constants with the active identity and select one descriptor before ingestion; they must not change scoring, corpus, qualification, frozen weights or query gold. The optional paid rerank arm is excluded from the decision and disabled on scratch; gate and degraded arms remain unchanged. No paid rerank calls are needed to select an embedder.

Gate B b3 measurements are explicitly post-hoc: B1 already consumed b3. B2 manifest on poc main specifies b3 SHA256 c220935ea1105cbee7d4838a5ab3e45ebac087f22d9d368b0667c063057205ec (38 queries), and Russian SHA256 ad8937ce0f72b361a48279593d29c242a5c055830a158aa3a41b68c73fa50214 (20 queries). Verify these before running. B1 metric is family recall: gold intersection with top K divided by min(K, gold family size), at 1/3/5. Preserve raw sums or per-query fractions to avoid selecting on four-decimal display rounding. No tuning of C10 or descriptor values after seeing recall.

Ordinary memory uses evals/mem-recall/benchmark.json and its mem-bench Recall@1/5/10, MRR and crosslang category. Source inspection found insert_raw does not index vectors: on scratch only, seed the same fixture then call reembed_bulk(true,false) before queries and fail on missing vectors or embed errors. This necessary embedding-enabled arm is run identically for all four models. It is labelled explicitly; no candidate is selected from a FTS-only tie. Never use a daemon or production store.

## Measurement commands

From the scratch merge root, with its own worktree-local target and offline model caches. `MEM_EVAL_MODEL` selects the global descriptor once at startup on scratch; no separate embedder is used by Blueprint recall. `HF_HOME` selects the prepopulated HF cache; make the cached L12 model available there before measurement, without changing weights.

```bash
export CARGO_TARGET_DIR="$PWD/src-tauri/target"
export HF_HOME=/Users/kandrey/.cache/huggingface/hub
export HF_HUB_OFFLINE=1
# Repeat with each model id from the table; one process per model/run.
MEM_EVAL_MODEL='<model-id>' cargo run --manifest-path src-tauri/Cargo.toml -p codeterm-mem --example blueprint_recall_eval --release -- --posthoc --queries evals/blueprint-recall/queries/set_b3.jsonl --label '<label>-b3' --out 'evals/mem-embedder/<label>-b3.json'
MEM_EVAL_MODEL='<model-id>' cargo run --manifest-path src-tauri/Cargo.toml -p codeterm-mem --example blueprint_recall_eval --release -- --posthoc --queries evals/blueprint-recall/queries/set_b3_ru.jsonl --label '<label>-ru' --out 'evals/mem-embedder/<label>-ru.json'
MEM_EVAL_MODEL='<model-id>' cargo run --manifest-path src-tauri/Cargo.toml -p codeterm-devtools --features dev-tools --bin mem-bench --release -- --fixture evals/mem-recall/benchmark.json --output-dir 'evals/mem-embedder/mem-bench/<label>' --report-dir 'evals/mem-embedder/mem-bench/<label>'
# A small eval-only oracle drives the real global provider, not another inference engine.
# Transformers AutoTokenizer/AutoModel generates the independent reference locally,
# from exactly the same cached revision, with official pooling/prefixes and normalization.
```

The candidate oracle and snapshot harness, added under this eval directory, must record exact executable commands and SHAs in their receipts. They may be registered as temporary scratch example targets to keep product architecture untouched. Cargo compilation uses Team utility shells and the build gateway policy; no shared final target directories.

## Snapshot protocol

Use only the Chair-provided read-only snapshot at blueprintlab/private/mem-snapshot. Never open the live memory store. Read source through SQLite read-only mode and copy into private temporary storage outside Git. Before any query-writing subject sees an item or any item reaches embedding, apply the product credential filter to all textual fields and body; exclude matches. Reports and committed artifacts contain aggregates only, never item text, ids, raw generated queries or raw vectors.

Sample 150 retained embeddable items, stratified proportionally by kind with deterministic seed 20261022. Fresh query writers see only one sampled item's title/summary; 120 write English and 30 Russian, with stable assignment before model measurement. Use Chair-authorized headless sessions, empty cwd and all CODETERM_* variables stripped, quota-gated at 85%. Query generation is currently externally paused; record any owner-approved provider change before use. Reject unusable responses and disclose missing counts rather than fabricate queries. Do not change queries after model results.

For each model build the same temporary filtered corpus, including any retained Blueprint library solutions; measure re-embedding time/items per second and recall@1/5/10 using both hybrid and vector-only paths. Report Russian subset separately, as well as full results and kind strata. Snapshot recall informs the disclosed trade-offs; the selection bounds below refer specifically to the fixed mem-bench and b3 Russian sets. Delete every worker-created snapshot copy and private query artifact after measurement; the Chair retains responsibility for the custody source. State actual deletion status.

## Selection rule (verbatim)

The winner is the candidate with the highest (a) recall@5 among those that lose at most 0.02 on mem-bench Recall@5 AND at most 0.02 on MRR AND at most 0.05 on Russian recall@5 vs L12.

- If the (a)-best candidate breaks a bound, state that trade-off explicitly next to the rule's pick.
- If no candidate beats L12 on (a), L12 stays the default.

Tie handling, declared before results: prefer L12 on equal Blueprint recall@5; otherwise use table order for exact eligible ties. Missing or failed required measurements mean no pick; do not silently exclude a failed model. Compare unrounded metrics, allowing only floating-point tolerance 1e-12 at bound equality. Explicitly disclose snapshot and crosslang losses even when all formal bounds pass.

## Sealed confirmation and delivery

Commit results and the rule's pick on feat/mem-embedder before opening, hashing or copying b4. Only then copy B2's b4, verify its SHA256 against B2 MANIFEST and confirm the winner exactly once using the same frozen C10. This is a new gate B attempt; gate B PASS requires family recall@5 >=0.80. Report a FAIL without another attempt. Record ATTEMPTS.md row text for the Chair; do not edit poc main.

```bash
MEM_EVAL_MODEL='<committed-pick-model>' cargo run --manifest-path src-tauri/Cargo.toml -p codeterm-mem --example blueprint_recall_eval --release -- --confirm --set b4 --queries evals/blueprint-recall/queries/set_b4.jsonl --manifest evals/blueprint-recall/queries/MANIFEST.json
```

After confirmation set the selected local default on the delivery branch. A focused fixture test covers the existing switch path from L12, item/entity/docs counts and successful search, with no network; a real local model migration oracle supplements it. If L12 stays, report the no-op default and still prove migration mechanics without claiming a model change. Preserve one provider and the existing stamped-index/switch architecture. Required focused checks: fmt, lane-check codeterm-mem --tests, test-one mem embedder_candidates, test-one mem embedder_default_switch, embedding-parity, authorized check-queue --lane, file size guard and ancestry checks. Inherited unrelated gate failures remain disclosed.
