# Stored history latency

Run against an existing frozen Bigcodex tab in canonical DEV. The bench only
reads older pages; it does not launch a provider or change the session. Use the
tab's exact session identity, the health build SHA, and a known byte cursor:

```bash
bash evals/perf/stored-history/run.sh TAB SESSION SHA BEFORE_OFFSET 40 target-check/evals/stored-history.json
```

Pass requires at least 20 distinct backward pages and p95 below 300 ms, including
HTTP serialization, transfer, and client JSON decoding. Every page must preserve
session authority and strictly advance its byte cursor. The output retains only
timings, byte ranges, and row counts. There is no cache warmup or repeated page.

DEV logs split `stored_snapshot` into `read_us` and `projection_us`. The
`runtimeStoredHistory` runtime-hook lines split argument preparation, VM wait,
execution, and decode. Existing plugin VM lines report lock contention. The
frontend `chat-scroll` diagnostic `backfill_merged` reports request and transcript
merge times for a page requested through the visible chat. Verify that the older
rows appear in the mounted transcript after a user scroll; API latency alone
does not prove that final DOM state.

Offline plugin comparisons use the same built-plugin Node VM runtime as the
provider contract tests, with no daemon or provider process:

```bash
node evals/perf/stored-history/offline.mjs ROLLOUT SESSION BEFORE_PLUGIN_JS AFTER_PLUGIN_JS OUTPUT_JSON
```

The rollout is opened read-only. The runner takes 40 descending 1 MiB windows,
discards the partial first record, alternates which bundle executes first on each
page, and asserts identical projected rows. It counts JSON.parse input bytes
outside the execution timer. No warmup or repeated page is used. Node VM timings
do not establish QuickJS performance; canonical DEV stage timers remain required.

S1a results on the frozen Bigcodex rollout, baseline `47fd18cae` versus
provider-codex 1.14.29, are retained in `offline-s1a-results.json`:

| Built plugin | Mean execute ms | p95 execute ms | Parsed bytes | Parsed lines |
|---|---:|---:|---:|---:|
| Before | 21.70 | 79.36 | 26,713,427 | 9,263 |
| After | 19.70 | 64.82 | 19,878,178 | 6,660 |

All 40 pages projected identical rows. The parsed-byte reduction is 25.6% for
these windows; their record mix differs from the full-rollout byte census.
