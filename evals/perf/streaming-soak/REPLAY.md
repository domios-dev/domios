# Saved-response replay and input profiling (COD-1184)

This extension uses the existing streaming-soak runner and resource journal.
Parent owns live execution on a disposable, named canonical DEV build. The
worker supplies offline tests and preparation only. Nothing starts/restarts DEV,
enables production CDP, launches a provider, or sends a provider message.

## Explicit functional replay without OS renderer attribution

Use `--purpose functional` on **every** invocation for one functional checkpoint.
This uses the same setup/replay/finish/cleanup and exact fixture ownership. It requires
the selected DEV/provenance/page/entry/daemon/boot identities but does not query or
select a browser-wide renderer PID. Identity and reports record renderer/trace
attribution unavailable with reason `functional_page_only`; renderer private bytes
are null. DOM/input/store/final-tail and page-CDP Performance metrics remain available.
Functional samples do not force GC or establish process memory/plateau/trace evidence.

For the already authenticated explicit Browser page, parent can run these finite
commands with a private output directory and previously prepared fixture. Replace
FULL_SHA and ENTRY_SHA256 with the exact fields from the first identity.json; do not
infer them from the caller checkout. Use a fresh output for each control/probe.

```bash
bash evals/perf/streaming-soak/run.sh inspect --purpose functional --surface chrome --cdp 58454 --target 16389F3D19EF145FF8D87BDE991C35E7 --out C:/private-evidence/functional-identity
bash evals/perf/streaming-soak/run.sh setup --purpose functional --surface chrome --cdp 58454 --target 16389F3D19EF145FF8D87BDE991C35E7 --out C:/private-evidence/functional-replay --mode replay --expect-build FULL_SHA --expect-entry ENTRY_SHA256
bash evals/perf/streaming-soak/run.sh replay --purpose functional --surface chrome --cdp 58454 --target 16389F3D19EF145FF8D87BDE991C35E7 --out C:/private-evidence/functional-replay --fixture C:/private-evidence/prepared-h320-c1024/replay-fixture.json --segments 1 --control stream --probe composer --profile none
bash evals/perf/streaming-soak/run.sh finish --purpose functional --surface chrome --cdp 58454 --target 16389F3D19EF145FF8D87BDE991C35E7 --out C:/private-evidence/functional-replay
bash evals/perf/streaming-soak/run.sh cleanup --purpose functional --surface chrome --cdp 58454 --target 16389F3D19EF145FF8D87BDE991C35E7 --out C:/private-evidence/functional-replay
```

The finite target/port above identify the parent's current admitted Browser, not a
persistent locator. A replaced page/boot/entry or capability change refuses resume.
`--purpose profile` remains the default and requires exact supported OS attribution.
Functional mode refuses CPU/trace profiles, snapshots, load/idle memory setup,
plateau analysis and before/after profiling comparisons. It does not silently turn
null renderer data into a successful memory oracle. Browser authentication and input
fixtures still require the already granted parent QA authorization; no extra pages
are created or closed to satisfy profiling assumptions.

### Bundled API-served functional preflight and evidence limit

On the SAME explicit parent-owned Browser page, first save its original URL, then
navigate through the normal Browser controls from `http://127.0.0.1:7686/` to
`http://127.0.0.1:7686/?e2e`. Do not create/close other pages or switch to Vite/HMR.
If no chat is mounted, open an already owner-approved existing chat through normal UI
so the shipped `__codetermTestChat` and `__chatProjection` handles exist. This does not
authorize input to that existing chat. Run functional `inspect` again and re-pin its
build, entry hash, exact page and `testMode:true` identity before setup. Setup refuses
missing URL mode/handles during attach, before creating owned fixtures.

Functional replay uses the existing full-message public test event with exact owned
synthetic IDs and backend-observed session/nonce/PTY checks before each seed/step.
It uses the shipped projection monitor and owned DOM source-message IDs; it NEVER
imports source modules into the bundled page. Internal transcript rows/authority are
not exposed by these public handles. Functional completion reports only the observed
owned projected tail signature and visible source ID. Internal full-history/content,
frontend authority and durable-storage evidence are explicitly unavailable. DOM or
projection evidence does not prove IndexedDB, localStorage, reload restoration or
durable transcript persistence. Those require separate already-supported storage QA.
Source-module/profile mode retains its stronger internal-store final-content oracle.

Use the functional command sequence above with the re-inspected SHA/entry and the same
explicit page ID. If a synthetic target is unmounted or does not expose its public
projection, dispatch refuses or the bounded observation fails; no success is invented.
After exact owned cleanup is confirmed, restore the saved original URL through normal
Browser controls (currently `http://127.0.0.1:7686/`). A failed/pending fixture journal
requires parent review before URL restoration; never replay or erase its evidence.

## Obtain and freeze the input

Use the supported `tab chat --since 0 --json` export of an owner-authorized saved
conversation. `sessions get` supplies metadata, not a transcript: never open its
provider history path or a provider database. Extraction is a parent action
outside bench scripts. If the approved source is production, identify that
explicitly in the parent shell before the read-only export. For example in that
shell (replace the tab and private path; do not run this inside an eval script):

```powershell
Write-Output 'Production read-only export of the owner-approved saved tab; no input'
codeterm tab chat --tab APPROVED_SAVED_TAB --since 0 --json |
  Set-Content -Encoding utf8 'C:\private-evidence\saved-export.json'
```

The source must be non-pending, have `count === messages.length`, and end with
the desired saved assistant answer. Parent must establish that this is a
finished saved turn: `pending:false` describes snapshot availability, not turn
completion, and this export cannot detect a still-growing answer. Missing rows,
opaque tail envelopes and unsupported exports refuse. Do not trim a failed
export and call it complete. A normalized export has no original ItemUpdated
chunk boundaries or timings: every replay is **reconstructed**, never a capture
of native/provider/SSE cadence. Preserve that limitation when investigating the
observed first-word-then-large-remainder symptom.

Prepare once, offline, without DEV or its wrapper:

```powershell
node evals/perf/streaming-soak/run.mjs prepare-replay --source C:/private-evidence/saved-export.json --out C:/private-evidence/prepared-h320-c1024 --history-rows 320 --chunk-bytes 1024
```

Use an existing private directory outside every checkout for source, prepared
fixture and evidence. The runner refuses paths inside its repository, including
symlink destinations. Artifacts contain private transcript data; keep them
untracked and protect the directory with the machine's private ACL. Prepared
fixtures/atomic reports use 0600 and new output directories 0700 where supported.
No source content is printed
or put in reports. Replay mode refuses heap snapshots because they can include
other tabs' prompts. `replay-fixture.json` is immutable by fingerprint; preserve
its exact bytes for both product versions and both surfaces.

Preparation retains every field of selected normalized rows except original
session/send correlation identity; row/source IDs and selected tool-call links
are remapped. Message content, Markdown/code, tool inputs/results and final tail
stay exact. `history-rows` selects the last N rows before the final answer and
records `omittedOlder`; it does not claim those omitted rows were replayed. The
default retains all exported history when it fits. Up to 400 history rows plus
the answer and synthetic seed fit below the existing 440-row transcript limit.
Use separately prepared 0/128/320/400-row variants when the export has enough
rows; never compare different variants as before/after evidence.

Chunk sizes are 4..65536 UTF-8 bytes, split only between code points. Source cap
is 8 MiB, total measured steps 2048, segments 1..8, transmitted normalized JSON
budget 256 MiB. Oversized workloads refuse before playback; choose a bounded
variant, do not raise caps or blindly retry a deadline. Prefixes grow under one
stable assistant ID, not a new row per chunk. Simple text segments reconstruct
losslessly; opaque envelopes refuse instead of dropping their data.

## Parent execution: pin the build and entry

If `inspect` refuses `identify --json`, verify the selected DEV instance's own
`bin/codeterm` artifact. The bench library ignores inherited `CODETERM_CLI_PATH`;
a production caller's CLI can have a different protocol. Repair the DEV CLI
through the existing build workflow, never route the bench through production.

Parent starts the named canonical DEV through the build/dev skills, enables its
existing frontend test mode (`VITE_CODETERM_TEST=1` at startup or the DEV-only
`?e2e` entry), verifies the actual health build, and forbids HMR during a run.
The runner stays locked to DEV API 7686. It uses the existing bench `dev_run`
wrapper, never inherited production selectors. It refuses PROD identifiers and
ambiguous renderer PID attribution in profiling mode.

```bash
bash evals/perf/streaming-soak/run.sh inspect --out C:/private-evidence/native-identity
bash evals/perf/streaming-soak/run.sh setup --out C:/private-evidence/native-before-stream --mode replay --expect-build FULL_SHA --expect-entry ENTRY_SHA256
bash evals/perf/streaming-soak/run.sh replay --out C:/private-evidence/native-before-stream --fixture C:/private-evidence/prepared-h320-c1024/replay-fixture.json --segments 3 --control stream --probe composer --profile both
bash evals/perf/streaming-soak/run.sh finish --out C:/private-evidence/native-before-stream
bash evals/perf/streaming-soak/run.sh cleanup --out C:/private-evidence/native-before-stream
```

`setup --mode replay` creates two separate synthetic transcript identities,
Soakchat/Soakother, and the existing idle owned terminal/emitter. It does not
require the memory bench's 40-minute idle declaration. It records exact session
nonces and uses the normal ownership/exit/cleanup guards. No saved source tab,
session, nonce, file or provider is rebound. Setup samples have no heap snapshot.

Run a fresh output/setup for every control and probe. Controls:

| Control | Measured background activity | Local response during measurement |
| --- | --- | --- |
| `stream` | Cumulative same-ID answer growth in Soakchat | Growing |
| `other` | Same growth in the separately owned Soakother | First prefix held |
| `mux` | Existing `__muxPerfChurnTab` frontend SET_STATE hook on owned Soakterm | First prefix held |
| `quiet` | Same number of acknowledged input/frame steps, no transcript/snapshot updates | First prefix held |

All controls seed identical selected history in both owned windows. Each segment
resets its owned draft and growing row to the same first prefix. After timing,
complete selected data in both windows is restored and verified. `mux` advances
frontend generation/status only; it is synthetic global-snapshot consumer work,
not authoritative daemon events. Use disposable DEV, and do not run providers in
these tabs. The terminal emitter remains idle; no `pulse` runs concurrently.

`--probe composer` sends one trusted CDP `Input.insertText` of `x` per step to
the owned focused textarea, never Enter. It verifies each input, stable focused
element and exact accumulated draft across controlled frame opportunities.
`other` refuses if the mounted test listener did not adopt the exact other tail;
it never pretends an unmounted/no-op event was delivered.

For `--probe mesh`, first focus Soakchat in that run and open the DEV Mesh
machines graph using the normal UI. Leave both owned chat windows mounted; the
runner refuses absent authority/listener surfaces. Mesh mode does not change
page selection. It locates the graph by its accessible `Mesh graph controls`
toolbar, verifies a visible uncovered point, and sends alternating wheel deltas
of +/-20 at that point. Run `quiet`, `other`, `mux`, then `stream` separately.
It counts graph DOM mutations and renderer frame intervals; these are not graph
React render counts. Source leads include MeshCanvas geometry/projection and
ResizeObserver work; useMeshData refreshes on mesh/service events. No graph
algorithm repair follows from source inspection alone.

Every live invocation has the existing eight-minute failure deadline and lock.
Setup waits for the owned chat root and accessible composer to mount. Replay
then checks the transcript store's exact source session and nonce before any
dispatch; the `snapshotReady` display attribute is not used as an authority
signal. The runner keeps its 20-second DOM evidence deadline. Each step waits
for input and explicit tail paint-opportunity/frame signals; no sleeps, polling
cadence or synthetic readiness delay. The pace follows the consumer's
acknowledgements. This does not recreate unbounded real-world burst arrival
pressure. Page instrumentation has a 450-second failure lifetime and stops
frame sampling when a sample budget fails; runner loss cannot leave an
unbounded sampler. Expiry removes the test handle and makes the run fail.
`pending` is persisted before replay; interruption refuses
replay and needs parent review of the ownership journal, not another run in the
same checkpoint. Successful replay is single-use. Complete/finish/cleanup each
owned run before the next control to keep mounted baseline counts fixed.

Cleanup subscribes to `mux-state-changed` before closing owned tabs/workspace.
Each bounded event triggers one `mux_get_state` read; cleanup completes only
after a shaped snapshot confirms every owned tab and the owned workspace are
absent. The 20-second observation deadline is a failure bound, not an API
latency measurement. A timeout leaves `cleanup.status: running`, does not mark
the checkpoint cleaned, and requires parent review; do not retry or edit that
journal.

## Native/Chrome and before/after

For Chrome use a parent-owned dedicated browser diagnostic session already
serving this DEV entry/test mode, authenticated through the normal UI, with an
explicit supported CDP endpoint/target. Profiling requires exactly one renderer;
functional mode records OS renderer attribution unavailable. No worker
launches it; no production browser is restarted. Add these same flags to
**every** inspect/setup/replay/finish/cleanup invocation for that run:

```bash
--surface chrome --cdp CHROME_CDP_PORT --target EXACT_PAGE_TARGET_ID
```

The native surface uses only the DEV-advertised CDP endpoint. A Chrome endpoint
must differ from that endpoint; native bridge presence must match the surface.
Identity includes browser product/version/User-Agent/V8/protocol, page path and
test-mode flag, full backend build SHA, served HTML/entry hash, boot/page IDs,
renderer/API-listener-owner PID and creation times. The listener owner is not
native-client CPU or lock attribution; this runner does not currently identify
the native wrapper PID. The entry hash is not a hash of all HMR
modules: record the exact loaded checkout/build/entry and harness SHA separately.

Repeat the same immutable fixture, chunk plan, history, controls, probe,
segments, viewport, mounted surfaces and profiler options on the after build.
Keep the same harness code for before and after even when product builds differ.
Compare like surfaces and controls offline (no DEV wrapper needed):

```bash
node evals/perf/streaming-soak/run.mjs compare-replay --out C:/private-evidence/native-stream-comparison --before C:/private-evidence/native-before-stream/replay-profile.json --after C:/private-evidence/native-after-stream/replay-profile.json
```

Changed workload fingerprints/options, incomplete runs or mixed surfaces refuse.
Compare native and Chrome reports side by side within each product version;
their difference is a separate surface control, not a before/after fix claim.

## Evidence and interpretation

`replay-profile.json` supplies per-segment p50/p95/p99/max/count/total for input
timestamp-to-capture, capture-to-document-bubble handler span, input-to-first and
second-rAF opportunities, continuous measured rAF intervals, normalized JSON
parsing and synchronous test-event dispatch-to-store return. Long-task entries
have the same distribution plus 20 bounded peaks. EventTiming queue/handler/
duration samples are present only when the renderer supplies eligible entries
for this target at >=16ms. No entries means unknown queue latency, not zero.

The bounded metadata timeline records normalized arrival/store-return time,
owned item ID/sequence, UTF-8 byte/character length, input/frame times and
diagnostic tail paint-opportunity time. It contains no response text, tool
arguments, prompts, token values or SSE payloads. `timeOrigin` and the timestamp
clock are recorded. Paint diagnostics and rAF are presentation opportunities,
not actual compositor presentation or OS input-queue measurements. Trusted CDP
input is not a physical keyboard or an IME proof; the focused ancestor wiring
test separately covers local draft/IME behavior.

CPU profiling uses 2ms statistical self samples, source/function groups and 20
top self frames. Trace collection caps at 200,000 events per segment, discards
arguments, and aggregates known renderer layout/style/paint/task/function events
with 20 peaks correlated to preceding renderer step markers. Durations overlap
and must not be added as CPU time. Markdown, projection, transcript model,
React/commit and Mesh groups are only as good as available source URLs and
function names; bundled/unknown frames remain `other`. No direct React Profiler
commit duration or isolated provider parsing/adoption duration is fabricated.
Store verification and instrumentation add work; use matching options and
`--profile none` controls when interpreting profiler overhead.

Existing post-GC `samples.jsonl` boundaries expose CPU seconds (Windows), heap,
private/working memory, DOM documents/nodes/listeners, mounted chats/windows and
xterm retention. Other platforms leave OS CPU/private counters unknown. Profile
windows and renderer Performance metric deltas cover measured steps; OS CPU
boundary deltas also include GC/restoration/sampling overhead. Evaluate repeated
fixed segments as trends, not an hours-long plateau result or a fixed latency
pass threshold. Unknown cache/backend retention remains explicitly unknown.

Completeness requires exact owned session/nonce, every selected normalized row
and final full-tail SHA-256 in both windows; a DOM marker alone cannot pass.
Changed backend session/nonce/PTY at the end refuses. Original saved source data
is never mutated. `complete:true` proves harness acknowledgements/completeness,
not acceptable responsiveness. Parent must report measured distributions,
long-task/CPU/retention trends and visible behavior separately.

To locate the first bunching boundary in the natural first-word/remainder
symptom, compare parent metadata-only provider/runtime/SSE arrivals where
supported against actual frontend listener/store/commit/frame observations.
Coarse upstream arrivals, queued frontend handlers, fast adoption with late
paint, and graph-only layout peaks are different findings. This reconstructed
post-provider replay bypasses Rust ItemUpdated production, SSE decoding and the
ordinary frontend authority gate; it cannot select among those upstream causes.
The WebView renderer and Rust host are separate processes; native client and API/server own separate Rust processes in split DEV. Healthy API/agents alone do not prove window responsiveness.

## Focused checks (offline only)

```bash
node scripts/affected-tests.mjs evals/perf/streaming-soak/run.mjs evals/perf/streaming-soak/runtime.mjs evals/perf/streaming-soak/io.mjs evals/perf/streaming-soak/model.mjs evals/perf/streaming-soak/replay.mjs evals/perf/streaming-soak/replay-model.mjs evals/perf/streaming-soak/replay-metrics.mjs evals/perf/streaming-soak/replay-page.mjs
node --test evals/perf/streaming-soak/model.test.mjs evals/perf/streaming-soak/oracle.test.mjs evals/perf/streaming-soak/heap.test.mjs evals/perf/streaming-soak/emitter.test.mjs evals/perf/streaming-soak/replay-model.test.mjs evals/perf/streaming-soak/replay-metrics.test.mjs evals/perf/streaming-soak/replay-page.test.mjs
```

Pure/controlled-DOM tests prove bounded planning, identity/final-tail refusal,
metadata privacy, event/frame wiring and aggregation. They are not live browser
timings. Parent first executes one bounded segment on a small prepared variant
to validate real CDP/wiring, then the unchanged saved-session cumulative workload
and controls. A failed live wiring run is a failure, not performance evidence.
