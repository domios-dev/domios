# agent-state — REAL-capture baseline scorecard (RT3/RT4)

The first **real** labeled corpus: live `claude` + `codex` agents driven through the
deterministic on-demand states by `capture-live.sh`, captured byte-exact to `.cast`
via the RT1 capture tap, replayed through the P1a `SignalExtractor` + each provider's
`classify_tab_state` by `cargo test --test agent_state_replay`.

These fixtures are the `*_real` dirs under `tests/fixtures/agent-state/<provider>/`.
They are **non-gating** (`"gating": false` in `expected.json`): MEASURED into this
baseline, never failing CI. Only the two curated synthetic seeds
(`codex/working_streaming`, `codex/update_prompt`) gate the build. A real
false-positive is **the finding**, not a failure — the oracle was never weakened to
hide one. P2 is graded against the numbers below and must strictly beat them.

## Scorecard (HEAD d851e32c, captured 2026-06-20)

```
fixture                            bucket truth              predicted            fW  fTC
------------------------------------------------------------------------------------------
claude/clarifying_question_real      REAL clarifying_question working               2    5  <- FINDING
claude/turn_complete_real            REAL unknown            working               2   15  <- FINDING
claude/working_streaming_real        REAL unknown            working               0    0  <- FINDING
codex/turn_complete_real             REAL unknown            working               0    0  <- FINDING
codex/update_prompt                  seed update_prompt      update_prompt         0    0
codex/working_streaming              seed working            working               0    0
codex/working_streaming_real         REAL unknown            working               0    0  <- FINDING

state                   precision     recall
--------------------------------------------
clarifying_question           NaN       0.00
unknown                       NaN       0.00
update_prompt                1.00       1.00
working                      0.17       1.00

false Waiting (mid-stream),       all:    4   real:    4
false turn_complete (mid-stream), all:   20   real:   20
```

`fW` = false Waiting (mid-stream), `fTC` = false turn_complete (mid-stream).

## Real false-positives found — the P2 targets

### P2-F1 — no terminal state: a finished turn reads as `working` (can't-tell-done)
**All 5 real captures** end at the provider's idle prompt (claude `❯`, codex `›`
composer) — the turn is genuinely DONE — yet `classify_tab_state` returns
`working` at the final quiescent frame. Both classifiers default to `working`
(`provider-claude/plugin.js:399,432`, `provider-codex/plugin.js:444`) and the
`TabState` enum has no `turn_complete`/`idle` member, so "done" is inexpressible.
This is why `working` precision is **0.17** (1 true seed / 6 predictions).
**Target:** introduce a done/idle distinction; a finished turn must NOT be reported
as actively working.

### P2-F2 — false `turn_complete` during pre-output thinking pauses
`claude/turn_complete_real`: **15** false turn_complete + **2** false Waiting
mid-stream. `claude/clarifying_question_real`: **5** false turn_complete + **2**
false Waiting. While claude "thinks" before emitting any token, byte-idle and
semantic-idle both cross the coarse two-clock thresholds, so the liveness gate flips
to "done"/"parked" even though output is still coming. This is the can't-tell-finished
/ premature-done bug, with receipts.
**Provider-specific:** codex shows **0/0** mid-stream FPs on the same triggers — it
streams more continuously, so its byte-idle never crosses the threshold. The bug is
concentrated in claude's think-then-burst cadence.
**Target:** real false turn_complete → **0** and real false Waiting → **0**.

### P2-F3 — prose clarifying questions are missed (false negative)
`claude/clarifying_question_real`: claude genuinely asked back ("Once I know what
\"it\" refers to, I'll fix it.") but in PROSE, not a structured option list.
`scoreClarifyingQuestion` only scores structured menus, so the verdict was `working`
(recall **0.00** for `clarifying_question`).
**Target:** decide whether prose ask-backs are in scope; if so, detect them — else
document that only structured option/permission prompts are classified and this is
expected.

## Coverage — captured for REAL vs needs-manual-trigger

| state | claude | codex | how |
|---|---|---|---|
| `working_streaming` | ✅ real | ✅ real | sustained ~400-word task, captured to completion |
| `turn_complete` | ✅ real | ✅ real | "reply with exactly OK" |
| `clarifying_question` | ✅ real (prose) | — | ambiguous "Fix it." — claude asked back |
| `presented_options` / permission | ⏳ manual | ⏳ manual | real tool-approval prompt (no auto-approve) |
| `waiting_on_auth` | ⏳ manual | ⏳ manual | spawn logged-out / `/login` |
| `rate_limited` | 🎲 opportunistic | 🎲 opportunistic | real limit banner; bank the sample |
| `update_prompt` | 🎲 opportunistic | ✅ seed (synthetic) | CLI self-update banner; bank the sample |
| `idle_stuck` | ⏳ manual | ⏳ manual | task blocks on stdin/network then goes silent |
| `error_crash` | ⏳ manual | ⏳ manual | hard error / kill mid-tool |

⏳ = **needs-orchestrator-manual-trigger** (interactive-TUI states; real system
machinery required — see `capture-live.sh recipes`, never faked in text).
🎲 = opportunistic (cannot force on demand). Layer-B Playwright UI oracle (RT5) is
ROG/Windows-owned and out of scope here.

## Reproduce

```bash
# capture (dev daemon must be up; DEV-LOCKED via evals/_lib/lib.sh `dc`)
evals/agent-state/capture-live.sh claude turn_complete_real "Reply with exactly: OK" 60
evals/agent-state/capture-live.sh claude working_streaming_real "Write approximately 400 words on the history of Unix pipes. Write it directly in your reply, do not use any tools." 150
# ...codex equivalents; see capture-live.sh header for the full matrix

# score
export CARGO_TARGET_DIR=/Users/user/Developer/.psetup-target
evals/agent-state/run.sh        # or: cd src-tauri && cargo test --test agent_state_replay -- --nocapture
```

<!-- revision: 1 -->
