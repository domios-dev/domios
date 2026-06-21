# Detection Provider Corpus Plan

Goal: every provider has two long full-session replays plus one question-focused
exploration replay. The replay, not the prompt, is the deterministic artifact.

## Required matrix

| Provider | Long session A | Long session B | Question/session UI |
| --- | --- | --- | --- |
| claude | `snake_incremental_build_browser_v1` | `systems_design_long_chat_idle_v1` | `terminal_questions_choice_v1` |
| opencode | `snake_incremental_build_browser_v1` | `systems_design_long_chat_idle_v1` | `terminal_questions_choice_v1` |
| codex | `snake_incremental_build_browser_v1` | `systems_design_long_chat_idle_v1` | `terminal_questions_choice_v1` + `codex_prompt_idle_regression_v1` |
| cursor | `snake_incremental_build_browser_v1` | `systems_design_long_chat_idle_v1` | `terminal_questions_choice_v1` |

`snake_incremental_build_browser_short_v1` is the gating-friendly version for
slow providers. It uses the first eight Snake turns, including the numbered-list
negative case, and avoids the expensive refactor / long-thinking turns. Full
`snake_incremental_build_browser_v1` remains the deep/debug recording.

## Recording order

1. Run the short Codex regression first. It validates the current false-Working
   fix before spending tokens on long captures.
2. Run `terminal_questions_choice_v1` once per provider. Promote only structural
   question frames: hidden cursor / selected numbered row / provider-native
   blocking menu. Plain A/B/C prose remains a negative.
3. Run the two long sessions per provider at `100x30` first. Use
   `snake_incremental_build_browser_short_v1` as the gating Snake replay when
   the full Snake scenario times out.
4. Add one shorter `160x40` geometry replay per provider after the 100x30 matrix
   is clean.

## Promotion gates

- `raw.cast`, `meta.json`, `phases.json`, `capture.log.json`, `screen.txt`, and
  `scenario.json` must all exist.
- `raw.cast` must contain input rows and output rows, and no malformed rows.
- Replay doctor must pass. Timed-out captures are debug-only, never gating.
- Idle holds must have zero `working`, zero `question`, zero `card`, and zero
  `native_picker` unless the phase is explicitly a question hold.
- `state=question` with `card=null` is allowed only for structural terminal-menu
  states. No provider copy is allowed in the detector.
- Before landing detector changes, rerender the existing timelines in place so
  old player URLs keep showing the latest logic.

<!-- revision: 2 -->
