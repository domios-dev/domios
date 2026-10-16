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

For a bounded ten-minute load, add `--seconds 600 --tabs 5 --records 16`.
The duration starts after warm-up; consumer acknowledgements drive each cycle,
and elapsed time only stops the workload. Timed mode samples post-GC at the
first completed cycle after each minute, with a final paused sample. Actual
sample times and cycle count remain in `result.json`; this short capture measures
growth and does not establish an hours-long plateau. Give the owning QA shell
enough remaining lifetime for warm-up, the workload and cleanup.

Use `--instrumentation minimal --snapshots no --allocations no` for the same
load and switching control without enabling CDP performance/heap profiling,
forcing GC or collecting snapshots. It reads `performance.memory` and process
private/working bytes; DOM/listener profiler counters remain unknown. These
heap samples include collectible allocations and are not post-GC retained size.

Every sample also records per-tab chat element/row counts, mounted terminal
normal/alternate/active buffer lengths when the DEV terminal handle is available,
canvas backing pixels and image intrinsic pixels. Missing terminal handles stay
unknown. Pixel counts are surface dimensions, not measured native allocations.
The user-agent memory API capability is recorded without invoking that potentially
expensive/GC-triggering API in the minimal control.

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

Add `--revisit yes` for a reverse visit to all warm tabs and three switches
between the first two, before appending new records. Each observation records
whether its ready chat DOM was already mounted before the click. Correlate
remounts with `chat_start.unchanged` log lines before calling them cache hits;
the DOM observation alone does not prove a cached attach.

`result.json` records each marker observation and switch latency, including
incomplete runs and cleanup failures. A completed workload is not a leak-free
verdict: compare equivalent warm cycles and cleanup, distinguish JS retention
from native renderer allocation, and account for profiler perturbation. A flat
DEV run alone does not establish that a different PROD workload is fixed.

Add `--bundle production` to open the daemon-served bundle at
`http://127.0.0.1:7686/` in an owned CDP page instead of the native Vite page.
The dev-locked bench library supplies the bearer token; credentials never enter
URLs or result files. The owned page closes during cleanup. Run both modes with
separate `--out` directories and the same fixture options for comparison.

Each visit records renderer clock boundaries (`rendererStartTime` and
`rendererEndTime`), `clientTiming.measures`, and stages grouped by request id.
Subtract the visit start from a request measure's `startTime` to see client work
before dispatch. The stages are:
`response` (request to arrival), `adopted` (arrival to store adoption), `painted`
(adoption to the first frame after a committed row has had a paint opportunity),
and `total` (request to that frame). `responseBytes` is the UTF-8 byte size of the
JSON-serialized decoded response, not compressed wire bytes. Two animation frames
observe a paint opportunity; they do not prove compositor presentation. Retained
visits without a request have empty measures. Instrumentation uses `DEV_BUILD`:
a daemon-served development bundle retains these marks with production React;
a release bundle strips them and reports empty client timing.

Pure aggregation check: `node --test evals/perf/chat-memory/measures.test.mjs`.

## Pure markdown allocation benchmark

Run in separate Node processes from the repository root:

```bash
pnpm exec tsx evals/perf/chat-memory/markdown-pipeline.mts --out baseline.json
pnpm exec tsx evals/perf/chat-memory/markdown-pipeline.mts --cached --out shared-parse.json
```

This uses the same fixture generator, message-part grouping, GFM parsing,
bare-domain links, code metadata, line breaks and ReactMarkdown conversion as
Chat. It models render-cache misses for 15 sessions with 24 distinct rows each,
visited three times (1,080 rows processed, 910,850 source characters). The working
set exceeds both existing 500,000-character caches, so this measures avoided
duplicate parsing even when tab switching churns those caches. It excludes DOM,
highlighting and renderer switch latency. Inspector sampling includes collected
objects at a 16 KiB interval; JSON records the largest self-allocation frames.

On Windows, Node 22.14.0, the paired run measured:

| Pipeline | Total sampled bytes | Bytes/row | Time/row |
| --- | ---: | ---: | ---: |
| Separate part/render parsing | 4,800,503,408 | 4,444,911 | 5.835 ms |
| Shared syntax, cloned before transforms | 2,804,344,136 | 2,596,615 | 3.342 ms |

Sampled allocations fell 41.6%; elapsed time fell 42.7%. An earlier pair measured
4.810 GB / 7.024 ms per row versus 2.809 GB / 4.911 ms per row. Timing varies with
machine load; neither measurement establishes a live Chat latency result.
The extra syntax cache admits at most 500,000 source characters, 3,000 entries,
and 50,000 characters per row. Character limits bound admission, not heap bytes.
Revisions use exact content keys; cached syntax is read by message grouping and
cloned before mutable render transforms. Message-part cache hits now refresh LRU.
Changed streaming content still parses in full once: a late reference definition
can change earlier blocks, which makes splitting an arbitrary tail unsafe.

The original `src-C9djCpjh.js` artifact at DEV b8af7b4bf maps by matching its
unminified function bodies to the locked dependency sources:

| Served location | Source |
| --- | --- |
| `58:28245` (`ed`) | `micromark-util-subtokenize@2.1.0/index.js:123`, `subcontent` |
| `66:1179` (`u`) | `unist-util-visit-parents@6.0.2/lib/index.js:335`, `visit` |
| `85:2378` (`o`) | `micromark-extension-gfm-autolink-literal@2.1.0/lib/syntax.js:112`, `tokenizeEmailAutolink.start` |

The pure profile reproduces those source frames. Both production consumers
previously called `remark-parse` → `mdast-util-from-markdown` with GFM extensions;
the shared parser eliminates the second tokenizer/autolink pass on a render miss.
Keel's live follow-up remains the original `run.sh` invocation with
`--allocations yes`, the integrated DEV SHA, and identical workload flags; compare
allocation volume, switch latency, stable marker DOM and post-GC retained heap.
