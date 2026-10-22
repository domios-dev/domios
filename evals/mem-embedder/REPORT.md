# BLU-14: L12 stays; b4 Gate B passes

The owner chose **keep L12** before b4 was accessed. The single real b4 confirmation passed: recall@1/3/5 = **0.8500 / 0.7000 / 0.8166666667**, against the Gate B recall@5 threshold of 0.80. The default remains one global L12 provider at 512 tokens and 384 dimensions.

PREREG: `c203284a4073054ee0a936c6f973f30611bf7182` (2026-10-22 17:57:25 +03), before every candidate measurement. Owner decision: `c013885e6692f8c3bf4fea2a1f740ab46b5852a6` (18:22:50 +03), before the first b4 payload access. Raw b4 results: `ca4cbb8a2a19a405f20fd2d1a095ef79491dcf0f`. Delivery branch: `feat/mem-embedder`.

## Candidate evidence

B3 (38 queries) and Russian (20 queries) are **post-hoc** family recall, using the fixed BLU-12 full-corpus harness and frozen C10. Each process selects one global provider before ingestion. No b4 candidate sweep took place.

| Model | b3 R@1 | b3 R@3 | b3 R@5 | Russian R@5 | Disk MB | Warm embeds/s | Oracle min cosine |
|---|---:|---:|---:|---:|---:|---:|---:|
| l12 | 0.842105 | 0.771930 | 0.921053 | 0.733333 | 479.725 | 117.36 | 0.999999821 |
| multilingual-e5-small | 0.894737 | 0.807018 | 0.868421 | 0.783333 | 487.726 | 109.99 | 0.999999881 |
| bge-small-en-v1.5 | 0.868421 | 0.833333 | 0.947368 | 0.333333 | 134.179 | 106.02 | 0.999999881 |
| e5-small-v2 | 0.973684 | 0.815789 | 0.929825 | 0.450000 | 134.179 | 105.63 | 0.999999881 |

BGE gains 0.026316 on b3 versus L12 and loses 0.400000 on Russian recall. E5-v2 gains 0.008772 and loses 0.283333. Multilingual-e5 gains 0.050000 on Russian but loses 0.052632 on b3. Disk MB is decimal and sums the five required cached model artifacts. Throughput is a short informational CPU sample: one warmup, 24 short public passage embeddings, with no concurrent compiler job. It is not a sustained performance benchmark.

Each candidate passed 70 production-path comparisons against local Transformers vectors for the same model revision, including passage/query prefixes, Russian, Unicode and truncation. All 280 comparisons exceed cosine >=0.999. References and receipts are retained; the initial multilingual offline cache failure produced no candidate vectors, and only unmeasured candidates resumed after the cache fix.

| Model | Evaluated HF revision | Pooling; query / passage prefixes |
|---|---|---|
| L12 | `e8f8c211226b894fcb81acc59f3b34ba3efd5f42` | mean; none / none |
| multilingual-e5-small | `614241f622f53c4eeff9890bdc4f31cfecc418b3` | mean; `query: ` / `passage: ` |
| bge-small-en-v1.5 | `5c38ec7c405ec4b44b94cc5a9bb96e735b38267a` | CLS; `Represent this sentence for searching relevant passages: ` / none |
| e5-small-v2 | `ffb93f3bd4047442299a41ebb6fa998a38507c52` | mean; `query: ` / `passage: ` |

L12 retains its accepted `main` descriptor; the cached evaluated revision is recorded above. The three added candidate rows are pinned. All use the existing Candle BertModel path, 512-token window and 384 dimensions. Multilingual-e5 uses its XLM-R tokenizer with a BERT checkpoint; vector parity proves this path. Pinned Python HF snapshots omit mutable refs; the loader now resolves a pinned SHA directly to its snapshot, with offline fixtures preventing redirection through a conflicting ref.

## Decision and superseded work

| Decision procedure | Verdict |
|---|---|
| Original PREREG: b3 best subject to ordinary Recall@5/MRR and Russian bounds | Incomplete: mem-bench was dropped. Of the measured b3/Russian arms, L12 is the only possible winner. English candidates violate the Russian bound; multilingual-e5 trails L12 on b3. |
| Intervening owner amendment: Russian reported, ordinary Recall@5 bound, L12 wins ties | Incomplete: ordinary mem-bench was dropped before a pick. |
| Final owner instruction, superseding both procedures | **KEEP L12**, committed before b4. |

Mem-bench, its crosslang category, snapshot recall, fresh query writers and the default switch/migration were **not performed** under the final owner scope. No ordinary recall safety claim is made. A temporary extra migration fixture passed once during the wider task and was then removed; this does not establish an actual model switch. No default change is needed. For any future default change, pin `legacy_identity()` to the L12 row and prove that unstamped L12 vectors are re-embedded rather than reused.

## Single b4 confirmation

B4 SHA256 `3d9da7cd5d10ff345b5628a18f61151c03138893aab3dd4ab51bb6f5d2a4ec6b` matched B2 MANIFEST after the owner pick commit. The fixed B1 scratch source was `270bdbb18b3b3b23e6934b4694d0db997682d534`; C10 SHA256 was `4dbb17ad4f8ccf306755a3ce707b0ac5adefd7d49428d741e9dff29896ffb432`. Scoring and C10 stayed byte-identical. The adapters only expose the active provider identity, select the one global provider before ingestion, and disable the optional paid reranker.

```bash
HF_HOME=/Users/kandrey/.cache/huggingface/hub HF_HUB_OFFLINE=1 \
MEM_EVAL_MODEL=sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2 \
MEM_EVAL_NO_RERANK=1 \
cargo run --manifest-path src-tauri/Cargo.toml -p codeterm-mem \
  --example blueprint_recall_eval --release -- --confirm --set b4 \
  --queries evals/blueprint-recall/queries/set_b4.jsonl \
  --manifest evals/blueprint-recall/queries/MANIFEST.json
```

One invocation, exit 0, 40 queries, timestamp `2026-10-22T15:24:17.102883+00:00`, gateway job `20261022T152352-58369-17577`. Gate R@5 = **0.8166666667: PASS**. Degraded R@5 = 0.7833. Optional rerank was not measured. [ATTEMPTS-b4.md](ATTEMPTS-b4.md) provides the exact append-only row for the Chair; no poc files were edited.

## Snapshot cleanup and branch boundary

The Chair snapshot was opened read-only and filtered before preparation. Aggregate preparation: 606 retained items, five credential matches and two short items excluded; 24 archived items were outside the source query. A 150-item sample was prepared, but zero subject sessions ran and no snapshot retrieval measurements ran. The entire worker-created private corpus/sample directory was deleted and absence verified. No private item text is retained in reports or committed artifacts. The Chair custody source remains for the Chair to delete. Neither the live production memory store nor its daemon was touched.

The local `eval/embedder-on-recall` scratch branch contains the real BLU-12 merge and is never delivered or pushed. `feat/mem-embedder` descends from accepted provider tip `12ad2c521628c24e8da9b8830c8f2213e552590d` and contains no `feat/blueprint-recall` commit; the ancestry command printed `separate`. Product changes are limited to the local model table, pooling helper and pinned-cache lookup with their fixtures. Trait, switch, CLI, UI and default are unchanged. Windows and Linux were not executed. Nothing was pushed.

## Focused verification

- `./build.sh lane-check codeterm-mem --tests`: exit 0 after the final pinned-cache source change. Compiles the product feature path and fixtures; it does not execute tests.
- `./build.sh test-one mem embedder_candidates --wait-build`: one test passed, exit 0, job `20261022T145927-4727-485`. This ran before the later cache fixture was added; that fixture was type-checked, not executed. The real offline candidate oracle supplies executed cache-loader proof.
- `./build.sh test-one mem embedder_default_switch --wait-build`: one test passed, exit 0, job `20261022T151247-26017-9104`, during the wider scope. The extra migration fixture was subsequently removed by the owner scope narrowing.
- `./build.sh embedding-parity --output evals/mem-embedder/l12-parity.json`: 25/25 historical L12 golden vectors passed, exit 0, job `20261022T153123-68759-11128`. The first invocation compiled successfully but exited 127 because the runner invokes bare `python`. One environment correction pointed a pad-local `python` link to the existing `/usr/local/bin/python3`; the same named operation then passed with offline model files. No product or global interpreter configuration changed.
- `./build.sh fmt-check`: refused before execution with exit 75 while the parity job held the gateway. No retry; defer this selector to canonical QA. The successful changed-file formatter is recorded separately.
- Earlier candidate test dispatch from a separate utility was refused before execution because its session nonce differed; the Chair authorized the single own-shell attempt that passed above.

The four-model Transformers oracle (280 cases), eight b3/Russian harness invocations and single b4 invocation all exited 0. Raw commands and source identities are in `oracle-receipts.json`, `recall-receipts.json` and `b4-summary.json`. An ordinary mem-bench build was canceled when the owner dropped that workload; it produced no benchmark result.

`./build.sh check-queue --lane` was attempted once on `d5cc0cd54759f90334fd6ef7de8780b5b7be22b5`, job `20261022T153237-69926-3742`, exit 75 before Cargo/Clippy: the shared check worktree could not publish because its path is not a working tree. This matches the inherited check-queue risk. No retry or shared-worktree repair was performed. Clippy is **not verified** by this lane. Canonical QA selectors requested: `./build.sh check-queue --lane` and `./build.sh fmt-check` after the owning tooling/environment is ready.

Final changed-file formatting: `./build.sh fmt`, exit 0 (`rust=6 biome=23 changed=no`). `bash scripts/check-file-sizes.sh --fail` and `git diff --check` passed, exit 0. The source diff adds no sleeps or timer-driven coordination. Ancestry checks printed `separate`. The final readiness report records the clean delivery tree and final SHA. No broad suite was run.
