# Two DEV mesh release evidence

Acceptance: spec `sp_apg34f`, section `Two DEV consoles over mesh`, checks 1/3/4 plus bidirectional mesh operations. This preparation contains an evidence journal, an 84-row matrix, bounded read-only capture, and shared offline helpers. It is not an automated UI acceptance runner. No live result has been claimed.

Stage 1 is source-only. Execute tests only after an ancestry-preserving merge in canonical under Orchestrator authority:

```sh
node --test evals/mesh-two-dev/*.test.mjs
```

Expected: matrix completeness, session confinement, preserved FAILs, immutable artifacts, target confinement, and event readiness tests pass. No daemon or LLM is needed.

After `GO live` supplies the exact meshqa id, the first live commands in canonical are:

```sh
node evals/mesh-two-dev/driver.mjs prepare target-check/evals/mesh-two-dev-B1
bash evals/mesh-two-dev/capture.sh EXACT_MESHQA_PEER_ID target-check/evals/mesh-two-dev-B1
```

Capture invokes only `dc` through the bench lib, selects aiderqa:7697, and reads meshqa only via `--mesh`. Each command has a 20-second failure deadline. No retry, fixed delay, build, restart, prompt, or pairing is performed. Every response or failure is preserved before proceeding. Use a new run directory for a retest.

## Remaining live procedure

1. Read `dc mesh discover`, verify the exact development identity, and run `dc mesh connect EXACT_MESHQA_PEER_ID`. The Orchestrator-provided BLACKSTAR utility shell approves the incoming request using its meshqa-scoped CLI. That local approval bootstrap is the only path outside aiderqa relay. Capture both `mesh status` responses and require each paired identity set to contain exactly the other DEV; never pair by advertised name alone. Stop on any PROD identity.
2. Discover provider catalogues on both consoles (`dc agent providers`, `dc agent models PROVIDER`, and their `--mesh` forms). Select only Claude haiku, Codex gpt-6-luna, OpenCode deepseek/deepseek-flash, Grok grok-4.6. MiMo adds rows only if configured. Do not substitute an expensive switch target; report an unavailable cheap switch as an unexecuted requirement.
3. Provision test workspaces and origin reporting tabs on both DEVs. From each console spawn each provider on the other, with a one-word title, description, explicit peer cwd and report target. Discover the spawn contract first. Record tab ids, bound session ids, provider, home identity and managed-by origin. Use pad-authored task bodies. The remote Workers must report home through their declared return route; verify the report is persisted on the origin and visible there.
4. Test `tab list --mesh`, send, get-text, report and paste put/get from both origins. Payloads belong in the caller pad and travel with `--from-pad`; preserve byte-exact paste roundtrip and its peer id. A send acknowledgement alone cannot pass the observed chat result.
5. For each tab run 15–30 medium turns through remote Chat and web Chat, including tool calls, code blocks and attachments. Exercise Stop, a catalogue-approved cheap model switch, and reload the test view. Record home/remote/web observations for all check-3 conditions. Fetch session metadata with `sessions get SESSION_ID`; collect the provider's raw persisted records on the home machine via its DEV utility shell and hand them through mesh paste. Retain the original path, exact bound-session metadata, record lines and raw artifact. A UI label or screen cannot prove model or turn claims.
6. Subscribe to home and remote status/activity events and instrument the rendered views. The shared `waitForEvent` helper accepts a cancellation-aware async event stream and fails on stream end/deadline. Capture Working and Idle/Done, sending dot, elapsed time and progress; correlate the same event ids on home/remote/web clocks using measured clock offsets. Every view must agree within approximately 1000 ms. Static sequential snapshots cannot prove this bound.
7. Restart aiderqa and meshqa separately only using the Orchestrator-owned named-DEV foreground utility shell and explicit canonical authorization. Never restart BLACKSTAR:7686 or reload its owner window. Await listener/lifecycle and reconnection events under a failure deadline; compare exact tab ids and ordered transcript/event ids before/after each restart. Close a test remote tab and require its absence in all views. Record ghosts, duplicates, missing or reordered events as FAIL with exact repro.
8. In each web direction switch through the titlebar peer picker, inspect the tab, reload that test browser view, then return through Gateway self. Manual rendered evidence is required; Playwright remains suspended. API transcript parity is supporting evidence only.

Per-OS read-only resource sampling argv lives once in `sampling.mjs`. Resolve each DEV PID from its exact listener before sampling; never guess the newest process. Leak check 5 and terminal check 2 are outside this B1 assignment.

## Recording results

`driver.mjs record RUN_DIR VERDICT_JSON` checks nonempty evidence artifacts, the full condition set for a PASS, and bound-session raw proof when attached, then appends `verdicts.jsonl` and renders `results.json`/`results.md`. `driver.mjs render RUN_DIR` rebuilds reports from that journal. Unexecuted rows remain `NOT_RUN` and are not acceptance PASS/FAIL. Every executed FAIL requires a repro and cannot later be overwritten by PASS in the same run.

Each verdict names direction, provider, check, view, status, artifact paths and observations. A FAIL repro must include exact steps, tab/session ids and log lines. A check-3 PASS must attach `providerProof`: `tabId`, `sessionId`, original `sourcePath` artifact, explicit `sessionPath` and `valuePath` arrays into each raw JSONL record, and `expected`. This generic extractor makes no claim about unknown provider schemas. SQLite-backed/non-JSONL providers still need a session-scoped home-machine export and format adapter; that integration is deferred, not silently inferred.

Remaining implementation: live pairing verifier, opposite-origin command/report driver, catalogue-bound spawn orchestration, provider-specific raw-record exports, rendered event collection and clock correlation, Chat/Stop/model/reload manual evidence ingestion, restart coordination and convergence oracle. Stage 2 must finish these slices and execute the matrix before reporting `in_review`.
