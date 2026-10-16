# Steady-state streaming soak (slices 1–3)

Fixture-only bench for the shared DEV daemon on 7686. It never starts a provider
or sends provider input. The terminal receives bounded synthetic batches over a
run-owned local pipe; the chat receives `fixtureRecord` JSONL in a fresh
provisioned workspace. Requires Node with WebSocket support, the existing bench
lib prerequisites, and a running native DEV with CDP and exactly one renderer.
Keel owns live execution. Blueprint scheduling and an actual hours-long run are
slice 4, outside this change.

## Commands

Run all live commands through the dev-locked wrapper from the repository root.
Use absolute output paths. Each invocation has an eight-minute failure deadline;
`pulse` accepts 1–450 seconds, reserving 30 seconds for final evidence. The pure
segment policy accepts at most 480 seconds. No process starts or restarts DEV.

```bash
bash evals/perf/streaming-soak/run.sh inspect --out /absolute/evidence/identity
```

Inspect writes `identity.json`: full build SHA, served HTML plus entry-script
SHA-256, boot/page IDs, renderer and daemon PID/creation. Copy its `build` and
`entryHash` into the setup flags. Every subsequent live invocation recomputes
and compares the complete identity before operating. HMR, restart, rebind, PID
reuse, PROD identifiers, ambiguous renderer attribution, or a foreign resource
cause refusal. The daemon PID comes from the actual 7686 listener; renderer PID
comes from the DEV browser's CDP process inventory. A checkpoint lock refuses
concurrent invocations instead of queueing.

First collect an idle control on this same build, frontend entry and process
lifetimes. The idle workload uses the same two mounted surfaces and focus
visits, with no additional transcript records or terminal output:

```bash
bash evals/perf/streaming-soak/run.sh setup --out /absolute/evidence/idle --mode idle --expect-build FULL_SHA --expect-entry ENTRY_SHA256
bash evals/perf/streaming-soak/run.sh pulse --out /absolute/evidence/idle --seconds 420
# Schedule subsequent finite pulses externally; at least 40 minutes of samples.
bash evals/perf/streaming-soak/run.sh finish --out /absolute/evidence/idle
bash evals/perf/streaming-soak/run.sh cleanup --out /absolute/evidence/idle
```

`finish` writes `idle-control.json`. Load setup requires this file **up front**
and derives each allowable bytes/hour slope from its measured regression and
early/late median slopes (the greater, clamped at zero). No arbitrary tolerance
can be supplied. Matching includes build, entry, platform, both process
identities, fixture count, terminal batch size, cadence and workload version.
An incomplete control is refused. Control samples and declaration stay in the
evidence directory; keep the original control with the load evidence.

```bash
bash evals/perf/streaming-soak/run.sh setup --out /absolute/evidence/load --mode load --control /absolute/evidence/idle/idle-control.json --expect-build FULL_SHA --expect-entry ENTRY_SHA256
bash evals/perf/streaming-soak/run.sh pulse --out /absolute/evidence/load --seconds 420
# Next invocation resumes the exact acknowledged batch and parent record.
bash evals/perf/streaming-soak/run.sh sample --out /absolute/evidence/load --snapshot yes
bash evals/perf/streaming-soak/run.sh finish --out /absolute/evidence/load
bash evals/perf/streaming-soak/run.sh cleanup --out /absolute/evidence/load
```

A pulse runs until its workload duration or 100,000 acknowledged cycles (whichever
comes first); `--max-batches N` lowers that cap. Each load cycle appends one
synthetic chat record, writes 16 terminal lines, proves the chat marker is in the
mounted DOM and the exact final terminal line reached the owned xterm buffer.
Pipe ACK alone is not the far-end pass. Readiness uses file publication,
MutationObserver, xterm `onWriteParsed`, socket responses and child `exit` events.
Elapsed time only schedules workload end, samples, and failure deadlines.
Output throughput follows actual consumer acknowledgements; it is not fixed-rate.

`finish` sends protocol stop, validates its exact batch number, then waits for a
receipt the supervisor writes from the child's `exit` event. It never waits for
inherited terminal pipes to settle. It preserves fixtures for inspection;
`cleanup` is the separate destructive stage. Cleanup revalidates identities,
sessions, PTYs, link target, file inventory and transcript hashes, refuses a
foreign tab in its workspace, and removes only the recorded tabs, fixture files,
link and workspace. Evidence files remain. No broad directory sweep or process
kill is used.

The checkpoint atomically records `pending` **before** any batch mutation. If an
invocation dies during a batch, resumption refuses to replay it. Preserve the
evidence for an owner decision; do not edit pending/batch/hash fields to bypass
the refusal. A stale `runner.lock` likewise needs an operator to verify its PID
has exited before removing that one lock. Interrupted setup/cleanup can leave
registered resources; review the checkpoint's resource journal rather than
deleting a workspace without matching its current contents. An interrupted stop
can resume from the supervisor's exact exit receipt.

## Sampling and verdicts

`samples.jsonl` records post-GC JS bytes, DOM nodes/listeners/documents, mounted
chats/xterms/canvases, transcript rows/windows, xterm retained lines/columns,
Tauri subscriptions/callbacks, and renderer/daemon private bytes and working set
with PID/creation. During a pulse the next acknowledged cycle takes a sample
once 60 seconds have elapsed; setup and segment boundaries also sample. External
segment gaps remain visible; they are never filled with zeroes or interpolated.

Current production modules expose no bench-safe census for markdown cache,
diagnostic rings, backend transcript retention or backend output retention.
Those named fields are explicitly `null` (unknown); the bench does not invent
instrumentation or change production. Subscription and xterm retention censuses
also return unknown when their existing handles are absent. Private bytes are
available on Windows; other platforms preserve working set and report private
bytes unknown, so a complete primary plateau verdict cannot pass there yet.

The plateau oracle reports each primary metric (post-GC JS, renderer private,
daemon private) as flat/rising/unknown. It requires at least 41 samples spanning
40 minutes, strictly increasing times, no gaps over 90 seconds, finite nonnegative
counters and post-GC evidence. Reports include linear bytes/hour slope, early
and late 20-minute medians, their bytes/hour slope, and the idle allowance. A
rising metric makes the result rising; otherwise any missing primary counter
makes it unknown. A short dry run is wiring evidence, never a plateau pass.
Unknown ancillary counters remain unknown even if the primary oracle is flat.

Offline reanalysis does not contact DEV:

```bash
node evals/perf/streaming-soak/run.mjs analyze --out /absolute/evidence/load
node evals/perf/streaming-soak/run.mjs diff --out /absolute/evidence/load --before /absolute/evidence/load/setup.heapsnapshot --after /absolute/evidence/load/finish.heapsnapshot
```

`heap-diff.json` groups changes by constructor and by constructor + retaining
path + immediate dominator path. It decodes V8 metadata, excludes weak edges,
finds shortest strong root paths and computes immediate dominators and retained
sizes across cycles/shared references. Paths have a 24-edge display bound with
an explicit ellipsis. Unreachable counts remain separate. Summed per-object
retained sizes overlap; only self bytes are additive. Snapshot analysis is
offline and may need a larger Node heap for large snapshots.

## Focused verification

```bash
node --test evals/perf/streaming-soak/model.test.mjs evals/perf/streaming-soak/oracle.test.mjs evals/perf/streaming-soak/heap.test.mjs evals/perf/streaming-soak/emitter.test.mjs
```

Pure tests cover identity/ownership/resume/cleanup/protocol/cadence decisions,
flat and rising controls, missing counters, paths, dominators, cycles and weak
edges. The isolated emitter test uses a temporary owned pipe, exact batches,
replay refusal, output counts, protocol stop and observed real process exit.

Keel's first integrated-build dry run: `inspect`, idle `setup --snapshot no`,
`pulse --seconds 1 --max-batches 1`, `sample`, `finish --snapshot no`, `cleanup`.
Pass conditions: exact build/entry identities retained; both surfaces mounted;
post-GC sample persisted; emitter exit code 0 from a child exit event; both owned
tabs/workspace absent after cleanup and fixture link/files removed. The primary
oracle must remain unknown for this short trace. Load-cycle far-end proof then
uses the measured matched idle control and `--max-batches 1` before the hours run.
