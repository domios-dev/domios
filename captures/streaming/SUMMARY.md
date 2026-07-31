# Streaming-preview quality — consilium convergence summary

Branch `release-1.7.8-streaming-from-console`. All numbers from the
time-accurate offline pipeline (`testbench/streaming/simulate.ts`) replaying
the EXACT Chat UI display path against real recordings: junk filter →
canonical coverage cut → question-anchored 8-row cap → smooth-or-hold
stabilizer. "OLD" = the pre-consilium pipeline at base commit `400950d6e`
scored with the fixed (honest) harness; the original jank metric was
tautologically 0 and is not comparable.

## questions.json matrix (fixed 7-prompt set, 120×32 unless noted)

| capture | jank OLD→NEW | leftover-turns OLD→NEW | dupe-frames OLD→NEW | height-shrinks OLD→NEW | lead NEW (min/avg/median/max ms) |
|---|---|---|---|---|---|
| codex          | 7 → **0**  | 3 → **0**  | 32 → **0**   | 0 → **0**  | +108 / +878 / +1050 / +1915 |
| opencode       | 66 → **0** | 14 → **0** | 3847 → **0** | 7 → **0**  | +1 / +592 / +613 / +1412 |
| codex-narrow80 (80×30) | 5 → **0** | 4 → **0** | 5 → **0** | 0 → **0** | +78 / +873 / +492 / +2810 |
| claude         | 52 → **0** | 6 → **0**  | 82 → **0**   | 12 → **0** | +301 / +3436 / +2366 / +12917 |

Lead compares preview birth against the turn's first canonical ASSISTANT
arrival (tool cards and user echoes are never the preview's comparison
target). Lead is positive on every measured turn of every capture.

Independently reproduced by ux-verifier's event-level scorer on all four
captures (plus live watches on running probes); prose-retention guard: codex
96% of canonical assistant segments appear in the preview (the one miss is a
benign markdown-link normalization gap); claude misses are pre-capture
bootstrap turns only.

## Interactive-prompt coverage (required addition)

| provider | preview renders question (criterion 1) | detection exactly-once (criterion 2) |
|---|---|---|
| opencode | PASS — full raise→answer→close round-trip: dialog previews, block dies with the answer, jank 0 / leftovers 0 / dupes 0 | PASS — one status transition (Waiting), one clean notification, preview events flow |
| codex    | pending capture | pending capture |
| claude   | **daemon gap — routed to eight main** (decoder emits zero events for the dialog; composer-cut excises the cursor-holding dialog; question state never fires) | **daemon gap — routed to eight main** |

Known cosmetic nit (daemon lane): the cursor-highlighted option row churns,
is classified volatile by the decoder, and is excluded from `turn_live`
before the display pipeline sees it — the dialog shows without that one row.

A pending (unanswered) question dialog displayed at capture end is correct
behavior, not a leftover: the user must see an open question until answered.

## What changed (matcher/display side, commits 400950d6e..HEAD)

- `simulate.ts` metric fixes: the smooth-transition check was tautologically
  true (every historical jank score was a fake 0); the scorer now replays the
  exact UI pipeline and adds dupes, height-shrinks, lead distribution, and a
  handover/leftover split (in-flight rows at a turn boundary that later land
  are handover, not stale junk).
- `streamPreviewMatch.ts` structural chrome filters (no provider ids):
  status/interrupt rows, composer echo, rules/borders, model·cwd and
  column-aligned footers, unframed diff/CLI-help panes, status badges,
  duration rows, tool-status morphs, thinking spinners (incl. hyphenated
  verbs) and completed verbs, progress-dot bars, queued-prompt footers,
  session banners, key hints, numbered file dumps, tool-run headers/children.
- `uncoveredPreviewTail` cuts through the DEEPEST canon-covered row, so one
  stray uncovered row can no longer pin landed content as a duplicate.
- Shared smooth-or-hold stabilizer (`stabilizePreview`): rows cut from the
  top never re-enter, rewrapped paragraphs re-anchor, any non-smooth
  transition holds the display, holds never outlive coverage, transient
  all-chrome frames hold instead of blinking. One shared
  `isSmoothPreviewTransition` drives both the display and the jank metric.
- Interactive questions: framed runs containing option rows are content
  (never junk), the window anchors at the question row, dialog open/close is
  a deliberate modal transition, dialogs bypass row retirement.
- 30 regression tests added to `streamPreviewMatch.test.ts` (38 total), one
  per real-world failure pattern fixed.

## Accepted residuals (evidence-based, invisible in real sessions)

- codex tool-summary bottom-collapse: 1 event in 1284 under EMPTY canon only;
  zero in every time-accurate replay.
- opencode long-prose mid-turn rewrap: ~6 sub-frame events under EMPTY canon;
  75ms frames absorb them (simulate 0).

## Product findings routed to eight main (daemon lane, out of scope here)

- `GET /api/events/stream` never emits `tab-status-changed` (recorder-2).
- claude interactive dialog: decoder composer-cut excises the dialog
  (motion_decoder.rs `turn_live_view` + blank-as-chrome), question state
  never fires — both criteria unfulfillable at the matcher layer.
- volatile-exclusion hides the cursor-highlighted dialog option row.
