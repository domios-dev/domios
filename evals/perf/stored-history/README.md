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
