# B-domios post-hoc full-corpus evidence

The confirmation remains **FAIL**: C10, L12, b3 pooled recall@5 **0.7982**. `results-b3.json` is unchanged. It ran once at `2026-10-22T14:18:37.308087+00:00`, before the harness defect was found.

The original harness inserted 69 qualified documents into one area under the product's 50-entry storage cap. It silently lost 19 entries. Commit `d1bbc4cf78de908a67c5dcfe2d8d2c25a3b144b2` spread the corpus across areas, recalled across all those areas, and required every inserted document to remain live. The frozen C10 configuration, save-time caches, gold and b3 queries were unchanged.

## Post-hoc results

Both artifacts below are exploratory, not confirmations. Each records source `d1bbc4cf78de908a67c5dcfe2d8d2c25a3b144b2` and the global L12 embedder. Neither changes the gate B verdict.

| evidence | timestamp UTC | gate @1/@3/@5 | degraded @1/@3/@5 | rerank @1/@3/@5 | rerank applied / paused |
|---|---|---|---|---|---|
| primary: `posthoc-b3-fullcorpus.json` | 14:35:00.015813 | 0.8421 / 0.7807 / 0.9211 | 0.8684 / 0.8246 / 0.9123 | 0.9211 / 0.8860 / 0.9474 | 36 / 2 |
| disclosed duplicate: `posthoc-b3-full-corpus.json` | 14:35:10.016758 | 0.8421 / 0.7807 / 0.9211 | 0.8684 / 0.8246 / 0.9123 | 0.9211 / 0.8333 / 0.9298 | 14 / 24 |

There are 38 b3 queries. Both runs ingested all 69 qualifying documents and applied 69 cached enrichments. `dx-log-rotate` has no cached agent intents and stays excluded. Dedupe accuracy is 0.9851 on 67 usable test pairs, with zero exact/structural verdicts on distinct pairs. The remaining three of the original 70 pairs involve the excluded document. The pre-registered starved original `echo` has zero b3 queries, so no starved-original recall estimate exists.

## Duplicate-run deviation

One post-hoc run was authorized. During the Claude-to-Codex provider handoff, the new session did not know that the predecessor's run was still executing, and launched another. The earlier artifact finished ten seconds before the duplicate. Chair selected the earlier artifact as primary and instructed that both be retained and disclosed, with no further b3 evaluation.

The deterministic gate and degraded arms agree. The rerank arms used separate model calls and experienced different quota pauses; their difference cannot be attributed solely to model nondeterminism. Paused calls retain the baseline fused order.

These post-hoc artifacts measure the harness fix at `d1bbc4cf7`. Subsequent product changes protecting saved entries, raising the storage bound to 1000, removing default recall truncation, and strengthening credential filtering are not a new b3 measurement.

## Reproduce stored tables

```bash
cargo run --manifest-path src-tauri/Cargo.toml -p codeterm-mem --example blueprint_recall_eval --release -- --report
```

This reads the stored grid, confirmation and post-hoc artifacts and prints pooled, per-original, dedupe and rerank-application tables without evaluating b3 again. No further `--confirm` or `--posthoc` invocation is authorized here. The next held-out confirmation belongs to BLU-14 on b4, which BLU-12 never opens.
