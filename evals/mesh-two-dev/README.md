# Two DEV mesh release evidence

Acceptance: spec `sp_apg34f`, section `Two DEV consoles over mesh`, checks 1/3/4 plus bidirectional mesh operations. This preparation contains an evidence journal, an 84-row matrix, bounded read-only capture, paired-only verification, opposite-origin operation plans, read-only provider exports, stream/rendered observation helpers, clock correlation and restart convergence oracles. It is not an automated UI acceptance runner. No live result has been claimed.

Stage 1 is source-only. Execute tests only after an ancestry-preserving merge in canonical under Orchestrator authority:

```sh
node --test evals/mesh-two-dev/*.test.mjs
```

Expected: matrix completeness, session confinement, preserved FAILs, immutable artifacts, target confinement, and event readiness tests pass. No daemon or LLM is needed.

After `GO live` supplies the exact windows id, the first live commands in canonical are:

```sh
node evals/mesh-two-dev/driver.mjs prepare target-check/evals/mesh-two-dev-B1
bash evals/mesh-two-dev/capture.sh EXACT_WINDOWS_PEER_ID target-check/evals/mesh-two-dev-B1
```

Capture invokes only `dc` through the bench lib, selects aiderqa:7697, and reads windows only via `--mesh`. Each command has a 20-second failure deadline. No retry, fixed delay, build, restart, prompt, or pairing is performed. Every response or failure is preserved before proceeding. Use a new run directory for a retest.

## Remaining live procedure

1. Read `dc mesh discover`, verify the exact development identity, and run `dc mesh connect EXACT_WINDOWS_PEER_ID`. Pairing approval is coordinated through Euler/Fermi using the owner-authorized Windows DEV utility surface; the tester reaches Windows only through aiderqa dc --mesh. If initial approval cannot be relayed while unpaired, preserve the refusal and ask Euler/Fermi to complete their locally owned bootstrap. Do not open a direct Windows connection. Capture both `mesh status` responses and require each paired identity set to contain exactly the other DEV; never pair by advertised name alone. Stop on any PROD identity.
2. Discover provider catalogues on both consoles (`dc agent providers`, `dc agent models PROVIDER`, and their `--mesh` forms). Select only Claude haiku, Codex gpt-6-luna, OpenCode deepseek/deepseek-flash, Grok grok-4.6. MiMo can add rows through the shared matrix helper only if configured; the CLI currently uses the four assigned providers. Do not substitute an expensive switch target; report an unavailable cheap switch as an unexecuted requirement.
3. Provision test workspaces and origin reporting tabs on both DEVs. From each console spawn each provider on the other, with a one-word title, description, explicit peer cwd and report target. Discover the spawn contract first. Record tab ids, bound session ids, provider, home identity and managed-by origin. Use pad-authored task bodies. The remote Workers must report home through their declared return route; verify the report is persisted on the origin and visible there.
4. Test `tab list --mesh`, send, get-text, report and paste put/get from both origins. Payloads belong in the caller pad and travel with `--from-pad`; preserve byte-exact paste roundtrip and its peer id. A send acknowledgement alone cannot pass the observed chat result.
5. For each tab run 15–30 medium turns through remote Chat and web Chat, including tool calls, code blocks and attachments. Exercise Stop, a catalogue-approved cheap model switch, and reload the test view. Record home/remote/web observations for all check-3 conditions. Fetch session metadata with `sessions get SESSION_ID`; collect the provider's raw persisted records on the home machine via its DEV utility shell and hand them through mesh paste. Retain the original path, exact bound-session metadata, record lines and raw artifact. A UI label or screen cannot prove model or turn claims.
6. Subscribe to home and remote status/activity events and instrument the rendered views. The shared `waitForEvent` helper accepts a cancellation-aware async event stream and fails on stream end/deadline. Capture Working and Idle/Done, sending dot, elapsed time and progress; correlate the same event ids on home/remote/web clocks using measured clock offsets. Every view must agree within approximately 1000 ms. Static sequential snapshots cannot prove this bound.
7. Coordinate each DEV restart separately with explicit canonical authorization. Windows DEV 7686 lifecycle is owned by Fermi and must be requested through Euler; the tester never runs its restart or rebuild. Never reload a BLACKSTAR desktop window. Await listener/lifecycle and reconnection events under a failure deadline; compare exact tab ids and ordered transcript/event ids before/after each restart. Close a test remote tab and require its absence in all views. Record ghosts, duplicates, missing or reordered events as FAIL with exact repro.
8. In each web direction switch through the titlebar peer picker, inspect the tab, reload that test browser view, then return through Gateway self. Manual rendered evidence is required; Playwright remains suspended. API transcript parity is supporting evidence only.

Per-OS read-only resource sampling argv lives once in `sampling.mjs`. Resolve each DEV PID from its exact listener before sampling; never guess the newest process. Leak check 5 and terminal check 2 are outside this B1 assignment.

## Recording results

`driver.mjs record RUN_DIR VERDICT_JSON` checks nonempty evidence artifacts, the full condition set for a PASS, and bound-session raw proof when attached, then appends `verdicts.jsonl` and renders `results.json`/`results.md`. `driver.mjs render RUN_DIR` rebuilds reports from that journal. Unexecuted rows remain `NOT_RUN` and are not acceptance PASS/FAIL. Every executed FAIL requires a repro and cannot later be overwritten by PASS in the same run.

Each verdict names direction, provider, check, view, status, artifact paths and observations. A FAIL repro must include exact steps, tab/session ids and log lines. A check-3 PASS must attach `providerProof`: `provider`, `tabId`, `sessionId`, `sourcePath` artifact and two distinct `expectedModels`. The driver derives 15–30 user turns and chronological model switch lines from the actual raw provider record. Other scalar claims use explicit `sessionPath`, `valuePath` arrays and `expected`. Unknown schemas are refused.

Offline slices now live in `identity.mjs`, `origins.mjs`, `home-export.py`, `provider-records.mjs`, `stream.mjs`, `http-stream.mjs`, `rendered.mjs`, `clock.mjs` and `convergence.mjs`. All await canonical execution; source is not a result. Stage 2 must wire the supplied origin utility shells, catalogue-bound spawns, actual provider paths/schema, renderer selectors and clock exchanges, and named-DEV authorized restart callbacks, then execute the matrix before reporting `in_review`.

## Export and observation adapters

The identity verifier consumes both `mesh status --json` and `mesh peers --json` on each side. Status counts alone cannot prove the paired set. Pin both ids, require exact assigned API ports and `local_dev`, and reject extra/offline peers or pending requests. The capture script records both endpoints through aiderqa.

`driveBoth` verifies pairing before it invokes either origin adapter. `originPlan` shares the same command construction for both directions; bodies always use pad-relative files. Each command has a 20-second failure deadline. The adapter runs on its declared origin, returns original command stdout/stderr plus proven origin identity/port and answered target identity. A windows adapter must be invoked through an Orchestrator-provided windows utility shell reached by aiderqa `dc ... --mesh`; it must never connect directly from the Mac to port 7686. The command driver records each capture or failure, obtains the paste id from its put response, and verifies the fetched body digest. The report request uses a pad-authored brief containing the child's own `agent report --from-pad` return command. `verifyReport` then requires exact body, bound session, sender/home id, receiving/origin id, persistence and visibility. Adapter-returned receipts need real origin evidence during Stage 2; fabricated envelopes are only offline fixtures.

Run the home export inside the appropriate named-DEV utility shell, then hand the immutable export and provenance through mesh paste:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 evals/mesh-two-dev/home-export.py BINDING_JSON OUTPUT_JSONL
```

`BINDING_JSON` carries `home`, `port`, `tab_id`, `session_id`, `provider`, `format`, raw `tab_snapshot` from `tab inspect --json`, and `metadata` from `sessions get --json`. Its existing absolute provider-owned `metadata.file_path` is the only source. Formats: `claude`, `codex`, `grok`, `opencode-sqlite`, or `session-jsonl` with explicit `schema.sessionPath`. OpenCode uses SQLite `mode=ro`, `query_only`, a single read transaction and parameterized session filters on every table. It exports original rows and rejects parts joined to foreign/missing messages. JSONL binds Claude sessionId or Codex session_meta id; no newest-file or cwd search occurs. A 16 MiB cap refuses rather than truncates. Outputs use exclusive creation and retain original source path and snapshot digest. Grok uses its own `~/.grok/sessions/<URL-encoded absolute cwd>/<session id>/updates.jsonl`. With `format: "grok"`, the exporter derives this exact source from the raw tab snapshot's cwd/session id; it never enumerates directories or selects the newest record. Grok session metadata is optional if that provider lacks `sessions get`; when supplied it must match the bound tab. An explicit `source_path` is accepted only under the provider's home sessions store and only when its decoded cwd/session directory matches the snapshot. Every update must carry the exact `params.sessionId`. `grok-records.mjs` reads selected model ids only from own `user_message_chunk` updates at `params.update._meta.modelId`, identifies turns by their persisted `promptIndex`, and counts multi-chunk prompts once. Hook/agent/catalogue updates cannot become user turns or selected-model proof. A model change requires the following own user turn, preserving the literal model id without alias normalization. `providerProof` includes `cwd` and the export provenance's `original_path` as `originalPath`. The synthetic fixture preserves the schema inspected in the supplied endurance record; it contains no live prompts or sensitive transcript text. Grok fixtures use grok-4.5 as historical switch evidence only; live Phase-B traffic remains restricted to the assigned cheap model catalogue.

Bounded read-only SSE capture after GO:

```sh
bash evals/mesh-two-dev/stream-live.sh RUN_DIR local-state.jsonl
bash evals/mesh-two-dev/stream-live.sh RUN_DIR remote-state.jsonl EXACT_WINDOWS_PEER_ID
```

These commands run via bench `dev_run`, use aiderqa gateway credentials and the documented signed relay stream for windows. Each capture has a 30-second failure deadline; state reads have a 10-second bound. Stream end/deadline produces a terminal evidence record and a nonzero exit. A capture is transport evidence, not an acceptance PASS. Raw SSE frames are written before validation; duplicates/gaps/reordering fail visibly, overflow forces an event-driven snapshot, and no retry/poll occurs.

`observeRendered` attaches a MutationObserver to the actual test view root, takes an initial sample, writes samples on mutations and returns a disposer. Stage 2 supplies selectors/read functions for status, sending dot, elapsed and progress plus an event correlation id. Those rendered artifacts stay distinct from stream receipts. Clock exchanges record local send/receive and remote timestamp, intersect offset intervals, reject clock-step inconsistency and require worst-case corrected parity within 1000 ms; midpoint guesses cannot pass.

`coordinateRestart` uses injected canonical utility authorization, snapshot, restart and event-reconvergence callbacks. It refuses every instance except aiderqa/windows and every wrong port, and requires an exact authorized gate. It has a 120-second overall failure deadline, preserves before/after/failure evidence and does not sleep or retry. `verifyRestart` requires a new boot plus listener-ready/reconnected events, all three views reconciled, exact stable tab/provider/session identities and ordered transcript ids, with no ghosts or stale closed tabs. It does not execute process control itself.

## Current attempt handoff

This pad copy belongs to attempt_4xgr2v. The source checkout already had an uncommitted retarget overlay; source-provenance.json and source-overlay.diff preserve its observed contents. No repository source was edited or committed by this attempt. Windows is a harness endpoint key, not an asserted peer name: bind the exact peer id from Euler's GO. targets.json pins aiderqa 7697 and BLACKSTAR Windows DEV 7686, with lifecycle requests routed via Euler to Fermi. No acceptance result or retargeted offline test result is claimed.

The pad launchers use the source checkout's bench lib and the pad's scripts. Keep that checkout available. From canonical, under Orchestrator authorization, run node --test against this pad directory's *.test.mjs; use the repository bounded runner. Live prepare and capture wait for GO, exact ids and canonical authorization.
