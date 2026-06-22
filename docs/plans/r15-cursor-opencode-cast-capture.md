# R15 — Capture procedure: missing Cursor / OpenCode agent-state `.cast` fixtures

Concrete procedure to record the missing `cursor` and `opencode` agent-state
replay fixtures, mirroring the existing `claude`/`codex` real-capture corpus
under `tests/fixtures/agent-state/`.

Scope (4 scenarios per provider):

1. `working_streaming_real` — sustained streaming output
2. `turn_complete_real` — short turn that finishes and parks at the composer
3. `clarifying_question_real` — prose ask-back (best-effort, non-deterministic)
4. `negative_prose_options_real` — assistant lists options **in prose**; classifier
   must NOT read it as a structured menu (`clarifying_question` / interactive card)

All four are **non-gating** real captures (`"gating": false`): MEASURED into the
baseline scorecard, never failing CI. A classifier false-positive is the finding,
recorded in `evals/agent-state/BASELINE.md`, not hidden by weakening an oracle.

---

## 0. Why this works without any code change

- `src-tauri/tests/agent_state_replay.rs::discover()` walks
  `tests/fixtures/agent-state/<provider>/<state>/` and picks up **every** dir that
  contains a `recording.cast`. It loads `plugins/provider-<provider>/plugin.json`
  per provider directory. Both `plugins/provider-cursor/plugin.js` and
  `plugins/provider-opencode/plugin.js` already exist, so dropping
  `tests/fixtures/agent-state/cursor/...` and `.../opencode/...` is auto-discovered —
  no harness edit needed.
- `evals/agent-state/capture-live.sh <provider> <state_label> <trigger> [timeout_s]`
  already accepts arbitrary providers (it just calls `dc agent spawn "$PROVIDER"`)
  and writes the fixture triple to
  `tests/fixtures/agent-state/$PROVIDER/$STATE/{recording.cast, screen.txt, expected.json}`.
  The state label you pass **is** the directory name — pass the `_real` suffix
  explicitly.

Binaries verified present on this machine:
`cursor-agent 2026.06.19-...`, `opencode 1.17.9`. (Note: the `cursor` binary is the
IDE shim; the agent provider drives `cursor-agent` — `provider_version()` in
`replay-capture.py` already prefers `cursor-agent`.)

---

## 1. Dev/prod safety (non-negotiable)

- `capture-live.sh` sources `evals/_lib/lib.sh`, which forces `dev_env` (port 7686,
  dev token, dev socket) and routes every CLI call through `dc` — `dc` asserts dev
  is alive on each call and re-exports the dev env so a leaked prod var
  (`CODETERM_API_PORT=7685`) cannot redirect to **prod**. HTTP helpers (`dev_get`)
  are hardcoded to `127.0.0.1:7686`.
- **Never** call bare `codeterm` in any capture shell. Use only `capture-live.sh`
  (or `dc` after sourcing the lib).
- Prod is port **7685** — never killed, never targeted. Dev is **7686**.

### Pre-flight (run once before capturing)

```bash
cd /Users/kandrey/Developer/codeterm-alpha

# 1. dev must be healthy and built from current HEAD
./build.sh kill
codeterm pane new --cmd "cd $(pwd) && ./build.sh dev"   # visible pane keeps the PTY owner
# wait, then:
curl -s http://127.0.0.1:7686/api/health | jq '.build'   # compare to: git rev-parse HEAD

# 2. providers must be authenticated NON-interactively (else the TUI parks on a
#    login/trust card that capture-live.sh cannot auto-dismiss — see Blockers).
cursor-agent --version      # must print a version, not a login prompt
opencode --version
#    If cursor-agent is logged out, log in MANUALLY in a normal shell first
#    (cursor-agent login / opencode auth login). Do this OUTSIDE the bench.
```

---

## 2. Exact capture commands

Run from repo root, dev up. Each command spawns a real agent in the scratch dev
workspace `WS-rt3-capture`, arms the RT1 `.cast` tap before the prompt, fires the
trigger, polls `/api/panes/<id>/status` until the turn settles, stops capture,
writes a starter `expected.json`, and closes the pane.

### Cursor

```bash
# 1) working_streaming — deterministic
evals/agent-state/capture-live.sh cursor working_streaming_real \
  "Write approximately 400 words on the history of Unix pipes. Write it directly in your reply, do not use any tools." 150

# 2) turn_complete — deterministic
evals/agent-state/capture-live.sh cursor turn_complete_real \
  "Reply with exactly: OK" 60

# 3) clarifying_question — best-effort (non-deterministic; see Blockers)
evals/agent-state/capture-live.sh cursor clarifying_question_real \
  "Fix it." 60

# 4) negative_prose_options — deterministic; force a PROSE option list, no tools
evals/agent-state/capture-live.sh cursor negative_prose_options_real \
  "In prose only, list three possible approaches to rename a file across a repo (call them option A, B, C). Do not ask me anything, do not run tools, just describe them and stop." 90
```

### OpenCode (identical matrix)

```bash
evals/agent-state/capture-live.sh opencode working_streaming_real \
  "Write approximately 400 words on the history of Unix pipes. Write it directly in your reply, do not use any tools." 150

evals/agent-state/capture-live.sh opencode turn_complete_real \
  "Reply with exactly: OK" 60

evals/agent-state/capture-live.sh opencode clarifying_question_real \
  "Fix it." 60

evals/agent-state/capture-live.sh opencode negative_prose_options_real \
  "In prose only, list three possible approaches to rename a file across a repo (call them option A, B, C). Do not ask me anything, do not run tools, just describe them and stop." 90
```

Each run prints `[rt3] done: <...>/recording.cast` and `ls -la` of the fixture
dir. Verify all three files exist (`recording.cast`, `screen.txt`, `expected.json`).

---

## 3. Label each `expected.json` (hand step — required)

`capture-live.sh` writes a **starter** `expected.json` with
`"final_state": "TODO_label_me"`. You MUST replay + read `screen.txt`, then set
`final_state` to the honest ground truth. Valid values are exactly the
`PaneState` snake_case variants (`src-tauri/src/agent/pane_state.rs`):

```
working | waiting_on_auth | update_prompt | rate_limited
clarifying_question | idle_stuck | unknown
```

There is **no** `presented_options`, `permission`, `turn_complete`, `waiting`, or
`idle` variant. `turn_complete`/`waiting` exist **only** as `negatives` entries
(the coarse-liveness axis, special-cased in `run_fixture`).

### Expected `expected.json` shape per scenario

`working_streaming_real` — ends parked at the composer; enum cannot express
"done", so the honest final verdict is `unknown`. The real measurement is
mid-stream (no false Waiting/turn_complete during streaming pauses):

```json
{
  "provider": "cursor",
  "final_state": "unknown",
  "gating": false,
  "source": "real-capture",
  "negatives": ["waiting_on_auth", "update_prompt", "rate_limited",
                "clarifying_question", "waiting", "turn_complete"],
  "notes": "Real cursor sustained ~400-word turn, captured start-to-finish, ending at the idle composer. Ground truth = streamed then done; enum has no turn_complete/idle, so final correct verdict is 'unknown'. HARD measurement is mid-stream: no false Waiting/turn_complete during streaming pauses. Any mid-stream FP is a wait-FP baseline finding -> BASELINE.md."
}
```

`turn_complete_real` — short reply then parks; same `unknown` ground truth:

```json
{
  "provider": "cursor",
  "final_state": "unknown",
  "gating": false,
  "source": "real-capture",
  "negatives": ["waiting_on_auth", "update_prompt", "rate_limited",
                "clarifying_question", "waiting", "turn_complete"],
  "notes": "Real cursor turn: 'Reply with exactly: OK'. Ground truth = turn finished, parked at composer. PaneState has no turn_complete/idle member, so honest verdict is 'unknown'. If the classifier returns 'working' at the parked composer, that IS the can't-tell-done finding -> BASELINE.md; do not relabel."
}
```

`clarifying_question_real` — only if the agent genuinely asked back. Ground truth
`clarifying_question`; classifiers that only score structured menus will miss a
prose ask-back (predicted `working`/`unknown`) — that false-negative is the
finding, not a relabel:

```json
{
  "provider": "cursor",
  "final_state": "clarifying_question",
  "gating": false,
  "source": "real-capture",
  "negatives": ["waiting_on_auth", "update_prompt", "rate_limited",
                "waiting", "turn_complete"],
  "notes": "Real cursor ambiguous task ('Fix it.'): agent asked back in PROSE, then parked. Ground truth = clarifying_question. Classifier scores only structured option lists, so a prose ask-back is expected to be missed; that false-negative is the finding -> BASELINE.md. If the agent did NOT ask back, this capture is INVALID — discard and retry (see Blockers)."
}
```

`negative_prose_options_real` — the assistant lists A/B/C in prose and finishes.
Ground truth = a finished turn (`unknown`). The point of the fixture is the
**negative gate**: the prose option list must NOT be classified as
`clarifying_question` at any mid-stream step:

```json
{
  "provider": "cursor",
  "final_state": "unknown",
  "gating": false,
  "source": "real-capture",
  "negatives": ["clarifying_question", "waiting_on_auth", "update_prompt",
                "rate_limited", "waiting", "turn_complete"],
  "notes": "Real cursor turn that lists options A/B/C in PROSE (no interactive card) and finishes at the composer. Ground truth = done -> 'unknown'. Negative gate: a prose option list must NOT be read as a structured clarifying_question/interactive prompt at any mid-stream step. A clarifying_question hit here is a false-positive finding -> BASELINE.md."
}
```

Repeat with `"provider": "opencode"` for the OpenCode set.

> Note on golden mismatches: because `gating:false`, a `final_state` mismatch (e.g.
> classifier returns `working` where truth is `unknown`/`clarifying_question`)
> prints `FINDING ...` and is recorded — it does **not** fail CI. Negatives that
> trip also only print findings. That is the intended baseline-measurement
> behavior; do not "fix" it by relaxing the label.

---

## 4. How to mark gating

- Real captures → `"gating": false` (default in the starter file is already
  `false`). They are measured into the scorecard / BASELINE.md and never fail the
  build (`agent_state_replay.rs` only `panic!`s for `gating:true && failures`).
- Synthetic curated seeds → `"gating": true` (or omit; default in the `Expected`
  struct is `true`). Only add a gating seed once a provider's classifier is
  expected to pass deterministically — NOT for these real captures.
- Keep the `_real` dir suffix so real captures never collide with future gating
  seeds of the same state name.

---

## 5. Score / verify

```bash
export CARGO_TARGET_DIR=/Users/kandrey/Developer/.psetup-target   # optional, matches BASELINE.md
evals/agent-state/run.sh
#   or: cd src-tauri && cargo test --test agent_state_replay -- --nocapture
```

The run prints the per-state precision/recall table plus
`false Waiting (mid-stream)` / `false turn_complete (mid-stream)` counts. The new
`cursor/*` and `opencode/*` rows appear automatically. Append the new rows + any
findings to `evals/agent-state/BASELINE.md` and bump its `<!-- revision: N -->`.

---

## 6. Blockers & nearest alternatives

### B1 — Cursor startup onboarding/trust card (HIGH risk)
`capture-live.sh` only waits for `status != Dead`, then sleeps 4s and arms capture.
It has **no** startup-prompt settling logic. `cursor-agent` can open on a trust /
onboarding / model-pick card; if so, the trigger text gets typed into that card and
the turn never streams — the capture is junk.

- **Primary mitigation:** authenticate and trust `cursor-agent` once, manually,
  before capturing (step 1 pre-flight), so a fresh spawn lands directly on the
  composer.
- **Nearest testbench alternative:** `testbench/replay-capture.py` already carries
  cursor-aware startup handling — `settle_startup_prompts()` (dismisses
  bracket-hotkey trust + numbered menus) and `recover_agent_after_startup()`
  (force-restarts cursor when it falls back to a shell prompt). It is dev-locked to
  7686 the same way. Drive a 1-turn scenario YAML through it
  (`python3 testbench/replay-capture.py <scenario>.yaml --provider cursor`), then
  convert its `raw.cast` + `screen.txt` output (under `testbench/replays/cursor/...`)
  into the fixture layout (`recording.cast` + `screen.txt` + hand-written
  `expected.json`). This is the deterministic-startup path when capture-live.sh
  stalls on cursor's onboarding. (It is also the right tool if we later want
  multi-turn or focus-churn captures.)
- **Do NOT** patch `capture-live.sh` to add cursor settling as part of this task
  (out of scope: no product/tooling edits, no fabricated casts). Flag it as a
  follow-up: port `settle_startup_prompts`/`recover_agent_after_startup` from
  `replay-capture.py` into `capture-live.sh` so cursor captures are one-command.

### B2 — `clarifying_question` is non-deterministic (MEDIUM)
"Fix it." only yields a clarifying question if the model chooses to ask back; cursor
and opencode models may instead guess and proceed, or invoke a tool. There is **no**
deterministic on-demand trigger and **no** synthetic generator for a prose ask-back.

- **Mitigation:** treat it as opportunistic — capture, replay, read `screen.txt`;
  if the agent did not actually ask a question, discard the dir and retry (vary the
  prompt, e.g. "Fix the bug." / "Update it."). Bank only a genuine ask-back.
- If after several attempts a provider never asks back, **do not fabricate** one.
  Record it in BASELINE.md as `— (not reliably reproducible)` for that provider,
  exactly like the claude/codex coverage table marks states that need a real trigger.
- The `negative_prose_options_real` scenario (B-independent, deterministic) still
  exercises the prose-vs-structured boundary even if the positive ask-back can't be
  banked, so the clarifying-question detector still gets a negative-side gate.

### B3 — OpenCode server/transport (LOW)
OpenCode has server-side machinery (`testbench/opencode-server-check.sh`,
`opencode-permission-gate.sh`). Spawn-as-PTY via `dc agent spawn opencode` is the
supported path and is what these scripts assume; if a capture stalls with no
streaming, check `testbench/opencode-server-check.sh` first to confirm the local
opencode server/auth is healthy before blaming the tap.

### B4 — Interactive-TUI states are out of scope here
`presented_options`/`permission`, `waiting_on_auth`, `rate_limited`, `update_prompt`,
`idle_stuck`, `error_crash` need real system triggers (`capture-live.sh recipes`) and
are NOT part of this 4-scenario task. Capture them opportunistically/manually later.

<!-- revision: 1 -->
