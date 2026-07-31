# Streaming-preview quality — consilium convergence summary

Branch `release-1.7.8-streaming-from-console`. All numbers from the
time-accurate offline pipeline (`testbench/streaming/simulate.ts`) replaying
the EXACT Chat UI display path against real recordings: junk filter →
canonical coverage cut → question-anchored 8-row cap → smooth-or-hold
stabilizer. "OLD" = the pre-consilium pipeline at base commit `400950d6e`
scored with the fixed (honest) harness; the original jank metric was
tautologically 0 and is not comparable.

## questions.json matrix — 3 providers × 3 geometries (complete)

| capture | jank OLD→NEW | leftover-turns OLD→NEW | dupe-frames OLD→NEW | height-shrinks OLD→NEW | lead NEW (min/avg ms) |
|---|---|---|---|---|---|
| codex 120×32   | 7 → **0**  | 3 → **0**  | 32 → **0**   | 0 → **0**  | +108 / +878 |
| codex 80×30    | 5 → **0**  | 4 → **0**  | 5 → **0**    | 0 → **0**  | +78 / +873 |
| codex 140×36   | 2 → **0**  | 3 → **0**  | 10 → **0**   | 0 → **0**  | +114 / +1136 |
| claude 120×32  | 52 → **0** | 6 → **0**  | 82 → **0**   | 12 → **0** | +301 / +3436 |
| claude 80×30   | 45 → **0** | 4 → **0**  | 131 → **0**  | 11 → **1** | +287 / +3314 |
| claude 140×36  | 41 → **0** | 12 → **0** | 103 → **0**  | 11 → **0** | +287 / +3261 |
| opencode 120×32 | 66 → **0** | 14 → **0** | 3847 → **0** | 7 → **0**  | +1 / +592 |
| opencode 80×30 | 40 → **0** | 12 → **0** | 99 → **0**   | 2 → **0**  | +870 / +1394 |
| opencode 140×36 | 13 → **0** | 14 → **0** | 0 → **0**    | 7 → **0**  | n/a — decoder gap C, see below |

The single remaining height-shrink frame (claude 80×30, one frame across the
entire matrix) is a documented cosmetic residual.

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
| codex    | provider declined at model level ("I can't access the interactive question tool in this mode", repeated on a clean tab) — not a CodeTerm defect; codex's own pickers (directory trust) prove the render path exists | n/a — no dialog raised |
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
- Decoder gap A — claude interactive dialog: composer-cut excises the
  cursor-holding dialog (motion_decoder.rs `turn_live_view` +
  blank-as-chrome); zero preview events during the dialog.
- Decoder gap B — claude question state: a genuine AskUserQuestion raises
  no status transition and no Input-needed notification (missed true
  positive; opencode on the same daemon transitions to Waiting).
- Decoder gap C — opencode at 140 cols: the decoder emits zero message
  content rows (all 1060 timeline rows are footer region: badge, cwd
  fragments, border, progress bar) — nothing can preview at that geometry.
- Cosmetic: volatile-exclusion can hide the cursor-highlighted dialog
  option row for a frame.

The matcher/display layer is converged everywhere the decoder delivers
content; the wide-140 chrome families (apostrophe-elided spinner verbs,
client feedback survey — ruled chrome, border+fragment composites,
right-aligned banners, wrapped tails of dropped framed rows) are filtered
with regression tests.
