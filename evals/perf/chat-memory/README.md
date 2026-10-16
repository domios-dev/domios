# Synthetic live Chat UI memory capture

Manual owner-authorized shared DEV capture. It refuses other ports and requires
the exact running binary SHA. No real provider is launched or messaged.

```bash
bash evals/perf/chat-memory/run.sh --expect-build FULL_SHA --tabs 15 --cycles 4 --records 64 --initial 480 --snapshots yes
```

The runner creates one owned workspace and new synthetic Claude-format JSONL
files, bound to plain shell tabs. A new provider-directory link is owned by the
run; existing paths are refused. Each cycle appends unique Markdown responses,
then visits every tab in the native DEV WebView through accessible controls.
The latest marker must reach the actual chat DOM. This exercises file parsing,
tailing, delivery, transcript retention and rendering, without a real PTY agent.
It does not reproduce provider RPCs or partial-token amendments.

Samples preserve JS heap, DOM/listener counts, mounted chat and terminal counts,
Windows private/working bytes and process creation identities. Warm and paused
snapshots are optional; compare them with the existing renderer-memory snapshot
summarizer. Samples before and after forced GC are labeled separately.
Add `--allocations yes` to sample V8 allocations during the measured cycles,
including objects collected by minor and major GC. `allocations.heapprofile`
retains the raw call tree; `allocations-summary.json` ranks self and inclusive
allocation volume by function and served source location. These are statistical
allocation estimates, not retained bytes or native renderer memory. Profiling
can perturb the workload; compare with the unprofiled control.
All created tabs are closed on exit. Real sessions, drafts and PROD are untouched.

`result.json` records each marker observation and switch latency, including
incomplete runs and cleanup failures. A completed workload is not a leak-free
verdict: compare equivalent warm cycles and cleanup, distinguish JS retention
from native renderer allocation, and account for profiler perturbation. A flat
DEV run alone does not establish that a different PROD workload is fixed.
