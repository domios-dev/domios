# Detection visibility — round 2: tool inventory, acceptance command set, regen list

Source of truth: `docs/plans/detection-rebuild-deepening-loop.md` (rev 5).
Round 1: `detection-visibility-findings-r1.md`.

## Build identity (this run)
- `git HEAD` = `3f477246`
- dev daemon `/api/health.build` = `3f477246` — **MATCHES HEAD** (dev was restarted
  since r1; the r1 stale-build `3c722440` mismatch is resolved).

## A. Tool inventory — who reads BAKED detector vs LIVE classifier

Production classifier = `src-tauri/src/agent/offline_state_classifier.rs`
(`classify_state` / `classify_timeline`) + `signal_extractor.rs`.

| Tool | Source of `detector.state` | Stale after Rust classifier change? |
|---|---|---|
| `src-tauri/examples/render_timeline.rs` | **LIVE** — calls `offline_state_classifier`, **writes** `detector` into timeline.json | n/a (it IS the generator) |
| `testbench/rust/state_doctor.rs` | **LIVE** — compiles `classify_timeline`/`render_signals_at` | No — reflects current source |
| `testbench/rust/state_calibration.rs` | **LIVE** — compiles `classify_timeline` | No |
| `testbench/replay-matrix.py` | regenerates (cargo build + run `render_timeline` on raw.cast) then analyzes | No — it regenerates first |
| `testbench/replay-doctor.py` | regenerates (`render_timeline()` builds + runs) | No — it regenerates first |
| `testbench/replay-analyze.py` | **BAKED** — reads `frame["detector"]` from committed timeline.json | **YES** |
| `testbench/replay-geometry-compare.py` | **BAKED** | **YES** |
| `testbench/acceptance-gate.py` (r1) | **BAKED** | **YES** |

Rule: **the BAKED readers report frozen verdicts.** After any change to
`offline_state_classifier.rs` / `signal_extractor.rs` / its extractors, run
`acceptance-suite.sh --regen` (or `replay-matrix.py`) before trusting the
python numbers. `state_doctor.rs` is the only no-regen live check.

## B. Minimal acceptance command set — `testbench/acceptance-suite.sh`

One command proves the four properties + prints build identity + transcript order:

```bash
testbench/acceptance-suite.sh           # gate over committed timelines
testbench/acceptance-suite.sh --regen   # re-render all casts first (after a Rust change)
testbench/acceptance-suite.sh --live    # + state_doctor (compiles classifier, no regen)
```

Result this run (daemon @3f477246): **GATE GREEN**, all codex transcripts ORDERED.

| Property | Check | Replays | Numbers (HEAD) |
|---|---|---|---|
| Normal answer → **final Idle** (codex/opencode/cursor/claude) | `final=idle && tail3s_working=0` | `provider_cli_question_probe_v1/*` (codex/opencode/cursor), `codex_prompt_idle_regression_v1`, `systems_design_long_chat_idle_v1/*` (incl. claude canonical) | 8/8 final=idle, tail3s_working=0 |
| Real terminal menu → **Question banner only** | `card=0 && native_picker=0`, question>0 | `terminal_questions_choice_v1/*` | codex q203, opencode q405, cursor q234/452; card=0 all |
| **Zero inline cards** | `card_frames==0` | ALL 46 timelines | card=0 everywhere |
| **Codex DOM order** | transcript seq/ts monotonic (data-layer proxy) | live `/api/panes/{id}/chat` | 3 codex panes, 0 inversions — see C |

Key frame ranges (provider_cli_question_probe_v1, ends idle):
- codex 1087f → idle f896–1087; opencode 885f → idle f694–885; cursor 1047f → idle f854–1047.
- codex_prompt_idle_regression_v1 955f → idle f950–955, idle-hold working_pct=0.0 (all turns).

## C. Codex ordering (P0 #2) — data layer is CLEAN; bug is frontend-only

`/api/panes/{id}/chat` returns the exact transcript the ChatUI renders
(`id,type,content,timestamp,seq`). Live codex panes:
- `2bace0d6` n=26 — **0 ts inversions, 0 seq inversions** (seq monotonic 0–25,
  strict chronological user/assistant alternation).
- `fd2b1b82` n=5, `ffa85297` n=13 — both 0 inversions.

⇒ The backend transcript is correctly ordered. Any visible out-of-order render is
a **frontend render bug** in `src/components/Terminal/PaneSessionChat.tsx`
(HEAD +57) — not the classifier and not the transcript data. Inspect its message
sort/key/group logic and the DOM node order.

**ChatUI DOM: NOT checked.** Playwright/CDP drives WebView2 and is
**Windows-only** (`tests/playwright/README.md`); this host is macOS (WKWebView,
no CDP attach). DOM-order confirmation must run on Windows, or add a macOS
WKWebremote-inspector path. The transcript proxy above is the macOS substitute.

## D. Timelines to regenerate after any Rust classifier change

**All 46** `testbench/replays/**/timeline.json` carry a baked `detector` and go
stale. All 46 have a sibling `raw.cast`, so all are regenerable:

```bash
testbench/acceptance-suite.sh --regen     # rebuilds render_timeline, re-renders every raw.cast
# or, with the existing matrix harness:
python3 testbench/replay-matrix.py
```

Canonical set that MUST be green post-change (the acceptance corpus):
- `*/provider_cli_question_probe_v1-question-probe-v1-{codex,opencode,cursor}/timeline.json`
- `codex/codex_prompt_idle_regression_v1-20260621-codex-prompt-idle-v1/timeline.json`
- `*/terminal_questions_choice_v1-*/timeline.json`
- `*/idle_focus_churn_v1-*/timeline.json`
- `*/snake_incremental_build_browser*/timeline.json`
- `*/systems_design_long_chat_idle_v1-*/timeline.json` (claude coverage)

Note: r2 frame counts for `idle_focus_churn` grew vs r1 (e.g. matrix-claude
1095→1510) — timelines were regenerated since r1; gate stayed GREEN across it.

<!-- revision: 1 -->
