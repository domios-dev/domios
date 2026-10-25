# Blueprint recall evaluation

Measures Blueprint library retrieval through the real global memory embedder and
temporary offline stores. This is a retained experiment, not an automatic release
gate. The runner and corpus were integrated in `f51a1acbba`; keep its results and
preregistration rather than deleting the directory to satisfy documentation checks.

## Preparation

Read [PREREG-bdomios.md](PREREG-bdomios.md), [POSTHOC.md](POSTHOC.md), the query
manifest and the frozen configuration before choosing an arm. Existing confirmation
results are sealed evidence. A new confirmation needs a new preregistered attempt;
never delete an old result or change its queries to obtain another attempt.

Run compilation only under the repository's build/gates procedures, in an authorized
Team utility shell with a worktree-local target. Run commands below from the repo
root. The example belongs to `codeterm-mem` and requires its `embeddings` feature.
Local retrieval needs the global model's populated cache; `HF_HUB_OFFLINE=1` prevents
an implicit model download. The harness uses disposable stores, not a live daemon.

## Inspect and reproduce

Render tables from committed measurement artifacts without opening a new query set:

```bash
cargo run --manifest-path src-tauri/Cargo.toml -p codeterm-mem \
  --example blueprint_recall_eval --features embeddings --release -- --report
```

With model weights available, inspect one ordinary retrieval query:

```bash
HF_HUB_OFFLINE=1 cargo run --manifest-path src-tauri/Cargo.toml -p codeterm-mem \
  --example blueprint_recall_eval --features embeddings --release -- \
  --query 'watch a failed build and notify its owner' --k 5 --json
```

The runner also accepts `--generate [--jobs N]`, `--grid`, `--freeze`,
`--confirm --set NAME --queries FILE --manifest FILE`, and
`--posthoc --queries FILE --label NAME --out FILE`. Follow the preregistration's
generate → select → freeze → single confirmation ordering for a new experiment.
`--generate` and the rerank arms in confirmation/post-hoc evaluation use live
provider sessions and quota checks; obtain explicit evaluation/budget authorization
before invoking them. Cached save inputs do not make those rerank arms offline.
Provider control and usage checks must use the dev-locked eval library for the
explicitly selected instance; never inherit a production CLI connection.

## Evidence and verdict

Retain the source SHA, embedder identity, corpus/query/config hashes, refused corpus
entries, enrichment counts and per-query scores. `grid.json` and
`config.frozen.json` describe selection; `results-<set>.json` is confirmation
evidence. `--posthoc` writes a separately labelled output and cannot replace a
confirmation. Report and query commands print their output to stdout.

The preregistered gate arm requires pooled family recall@5 at least 0.80; report
degraded and reranked arms separately. A missing input, unavailable model, quota
pause, refused confirmation or nonzero exit is incomplete evidence. Existing
historical results do not prove the current head passes. No model run or provider
session is required to verify this README's presence in the documentation gate.
