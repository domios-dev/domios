# Detection visibility — round 3: unified table, staleness, Stop/Cancel/Esc, ordering gate

Source of truth: `docs/plans/detection-rebuild-deepening-loop.md` (rev 5). Prior: r1, r2.
dev daemon this run: `3f477246` (= git HEAD). NOTE: `offline_state_classifier.rs`
and `render_timeline.rs` have **uncommitted** working-tree edits (Loop Lead R3/R4).

## 1. Unified oracle table — `testbench/oracle-table.py`

One row per timeline: final state, tail3s_working, question tail, mid-build question,
card frames, frames, STALE flag. (`--canonical` = acceptance corpus, `--json`.)

```
python3 testbench/oracle-table.py --canonical
# 26 timelines | STALE=26 fresh=0
  final tail3sW qTail midQ cards frames  st  name
   idle       0     0    0     0    955  ST  codex_prompt_idle_regression_v1
   idle       0     0    0     0   1087  ST  provider_cli_question_probe_v1 ...codex
   idle       0     0  256     0   1047  ST  provider_cli_question_probe_v1 ...cursor   (real Q surface)
   idle       0     0  249     0    885  ST  provider_cli_question_probe_v1 ...opencode (real Q surface)
question      0    41  245     0   1573  ST  terminal_questions_choice_v1 ...claude    (ends on open Q)
question      0    41  405     0   1214  ST  terminal_questions_choice_v1 ...opencode  (ends on open Q)
   idle       0     0    0     0  10341  ST  snake ...codex
working      41     0    0     0  22597  ST  snake ...canonical (capture window cut mid-build)
   ... (full 26 in tool output)
```

Reading: **cards=0 in every row.** `final=question` rows are
`terminal_questions_choice` replays that legitimately END on an open terminal
question (correct: Question banner, card=0). `qTail`/`midQ` high on cursor/opencode
probes = real question surfaces (correct). The one `final=working` is the snake
canonical capture whose window was cut before the build finished (capture artifact,
not stuck — midQ=0, cards=0).

## 2. Staleness after the latest Rust change

Production classifier source (`offline_state_classifier.rs`) mtime **18:37:14**.
Newest committed `timeline.json` predates it.

**ALL 46 timelines are STALE; 0 were regenerated after the change.** The baked
`detector.state` the python readers consume is PRE-R3/R4. Before trusting any
python gate number against the current classifier:

```bash
testbench/acceptance-suite.sh --regen     # rebuild render_timeline + re-render all 46 casts
# then: python3 testbench/oracle-table.py --canonical   (STALE column flips to ok)
```

`state_doctor.rs` / `state_calibration.rs` already reflect the edited classifier
(they compile it) — use those for a no-regen sanity check.

## 3. Stop / Cancel / manual-Esc -> Idle — API oracle (macOS, no DOM)

`testbench/cancel-oracle.sh` — pure `/api/*`, runs on macOS WKWebView (no WebView2).

| action | endpoint | effect |
|---|---|---|
| Stop button | `POST /api/panes/{id}/stop` | writes provider `stop_keystroke()` (ESC for claude/codex/opencode; cursor Ctrl+C; None hides Stop) |
| manual Esc | `POST /api/panes/{id}/send {"actions":[{"key":"esc"}]}` | `keys.rs`: esc -> `\x1b` |
| Cancel prompt | `POST /api/panes/{id}/prompt/clear` | clears pending question |

Oracle flow: drive pane to Working (send prompt) → confirm `agent_state.state==working`
→ issue action → poll `/api/panes` until `state==idle`, report latency-ms + PASS/FAIL.

Status this run: **wiring-verified** on idle disposable pane `060a33cd` — `/send esc`
reachable, idle stays idle, state polling works. Full live drive is ready but needs a
**disposable** agent pane (I did not drive Andrey's/Loop-Lead's panes):
```bash
testbench/cancel-oracle.sh --pane <disposable_id> --mode stop   # or esc / cancel
```

## 4. Codex ordering — deterministic data-layer gate (`testbench/chat-order-gate.py`)

Asserts the transcript `/api/panes/{id}/chat` (the data the ChatUI renders) is
seq-strictly-increasing, timestamp-monotonic, no duplicate ids.

```bash
python3 testbench/chat-order-gate.py testbench/fixtures/codex-chat-order-2bace0d6.json  # frozen fixture, deterministic
python3 testbench/chat-order-gate.py --live --agent codex                              # all live codex panes
```

Results this run: fixture (n=26) **GREEN**; live codex panes `ffa85297`(13),
`2bace0d6`(26), `fd2b1b82`(5) all **GREEN** — 0 seq/ts inversions.

⇒ Codex transcript is correctly ordered at the data layer. **P0 #2 is a
frontend-only render bug** in `src/components/Terminal/PaneSessionChat.tsx`, not the
transcript/classifier. **DOM render order remains MANUAL / Windows-only** (Playwright
CDP → WebView2; macOS WKWebView has no CDP attach). Frozen fixture lets the gate run
in CI without a desktop.

## New tooling added this round
- `testbench/oracle-table.py` — unified table + staleness column.
- `testbench/cancel-oracle.sh` — Stop/Cancel/Esc → Idle API oracle (macOS).
- `testbench/chat-order-gate.py` — deterministic transcript ordering gate.
- `testbench/fixtures/codex-chat-order-2bace0d6.json` — frozen ordering fixture.

<!-- revision: 1 -->
