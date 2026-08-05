# ux-verifier FINAL SIGNED TABLE — streaming preview quality

## questions.json matrix: 3 providers x 3 terminal geometries (9/9 cells)

| capture | geometry | jank | dupes | leftover-turns | height-shrink | lead min/avg (ms) |
|---|---|---|---|---|---|---|
| codex | 120x32 | 0 | 0 | 0 | 0 | +108 / +878 |
| codex-narrow80 | 80x30 | 0 | 0 | 0 | 0 | +78 / +873 |
| codex-wide140 | 140x36 | 0 | 0 | 0 | 0 | +114 / +1136 |
| claude | 120x32 | 0 | 0 | 0 | 0 | +301 / +3436 |
| claude-narrow80 | 80x30 | 0 | 0 | 0 | 1 | +287 / +3314 |
| claude-wide140 | 140x36 | 0 | 0 | 0 | 0 | +287 / +3261 |
| opencode | 120x32 | 0 | 0 | 0 | 0 | +1 / +592 |
| opencode-narrow80 | 80x30 | 0 | 0 | 0 | 0 | +870 / +1394 |
| opencode-wide140 | 140x36 | 0 | 0 | 0 | 0 | n/a (decoder gap C) |

## Interactive-prompt coverage (required addition) — 3 distinct outcomes on ONE daemon

| provider | criterion 1: preview renders the question | criterion 2: detection exactly-once true positive |
|---|---|---|
| opencode | PASS — dialog previews with the question row visible (question-anchored window); full raise→answer→close round-trip jank 0 / dupes 0 / leftovers 0 | PASS — one `working→question` transition, ONE `Input needed` notification with clean structured text |
| claude | DAEMON GAP A — 0 preview events for a genuinely rendered AskUserQuestion dialog (decoder excises the cursor-holding dialog) | DAEMON GAP B — status hand-polled `Idle` throughout; no notification = MISSED true positive |
| codex | PASS (degraded form) — picker unavailable in codex "Default mode", question degrades to prose which previews cleanly: jank 0 / dupes 0 / leftovers 0, lead +182 ms | n/a — no picker raised; PROVIDER MODE GATE, not a CodeTerm defect (3 attempts, 2 tabs, with and without charter) |

## Live sign-off on the shipping binary

- `/api/health` build_sha `284d3303d66a9beae4237dab5cbb35ffbe8ad8a8` == branch HEAD — assertion PASS (no stale-binary trap).
- Identical live prompt to all three providers on the rebuilt binary (restored producer + locked-spec float):

| provider | jank | dupes | leftovers | lead | peak rows |
|---|---|---|---|---|---|
| codex | 0 | 0 | 0 | +211 ms | 4 |
| opencode | 0 | 0 | 0 | +189 ms | 6 |
| claude | 0 | 0 | 0 | +2529 ms | 2 |

- Preview blocks born mid-stream and cleared at turn end (4 births / 14 death events, no leftovers).
- ZERO notifications across all three working turns — the #283 flip-side holds (working turns silent).
- Float panel verified structurally: zero-height anchor + absolutely positioned fixed 9.5rem panel ⇒ chat layout cannot shift; `overflow-hidden` + tail anchoring ⇒ panel never resizes; status header renders before the first row; scroll button raised to z-30 so it is no longer occluded.

## Daemon-level gaps routed to eight main (#283) — NOT matcher-fixable

- **A** claude question dialog produces zero `chat-stream-preview` events (decoder composer-cut excises the cursor-holding dialog).
- **B** claude question-state never detected → no `Input needed` notification (missed true positive).
- **C** opencode at 140×36: decoder emits ONLY footer rows (1060 rows / 11 shapes / 0 content hits) while the cast proves content was on screen (fizzbuzz ×135, scrollback ×227); same provider at 120 cols yields 23 737 content hits.
- **D** opencode question-state FALSE POSITIVES: 5 `Input needed` notifications during ordinary streaming turns, bodies are screen-scraped prose fragments; status flaps `idle→question` mid-stream.

## Methodology integrity notes

- The original jank metric was tautologically 0 (`isSmoothTransition` allowed empty `kept`); every pre-consilium "0 jank" was fake. Fixed before any baseline was trusted.
- My scorer now uses the shipping `isSmoothPreviewTransition` and retains a `strict_raw_only_jank` tripwire for classifier divergence: **0 across all nine cells**, so adopting the shipping classifier laundered no findings.
- Two earlier alarms of mine were retracted after investigation: simulate's "leftover turns" (turn-attribution artifact) and claude "covered-content leftovers" (my end-state-blob vs the UI's rolling 20-message window).

**SIGNED: shipping-accurate for the streaming preview display path at branch HEAD 284d3303d.**
Residual accepted: 1 cosmetic height-shrink frame (claude 80×30, turn 6). Gaps A–D are daemon-side and excluded from this signature by ruling.

## Addendum: activity-bar redesign (96e55e929) — re-verified live on the rebuilt producer

The overlay panel was replaced by a slim in-flow 34px single-line activity bar
(last child of the message list). All five redesign criteria pass:

1. No overlap — in-flow by construction (rendered inside the scroll content, after `PendingPromptCard`); the whole z-index/occlusion defect class is gone.
2. Constant height / never stacks — outer and inner heights pinned to 34px; the line is `truncate whitespace-nowrap`, so a 239-char line (measured, claude) truncates rather than wrapping.
3. Accent sweep animates; `prefers-reduced-motion` swaps to a static opacity pulse.
4. Collapses 34px→0 with opacity and unmounts on `onTransitionEnd`.
5. No mid-stream chat jump — height is invariant while streaming; only appear-at-start and collapse-at-end change it.

Live single-line behaviour (metric refitted to the bar — replay the shipping
pipeline, track `rows[last]` only), on build 284d3303d:

| provider | events | line changes | repeats of an older line | empty flashes | longest line |
|---|---|---|---|---|---|
| codex | 23 | 5 | 0 | 0 | 116 |
| opencode | 57 | 40 | 0 | 0 | 115 |
| claude | 31 | 7 | 0 | 0 | 239 |

The bar never goes backwards and never blanks mid-stream. Matcher metrics
unchanged in the same pass: 0 jank / 0 dupes / 0 leftovers per provider, leads
+98…+9237 ms, zero notifications on working turns.

Two non-blocking findings filed to matcher-analyst:
- **A11y**: `role="status"` wraps the rapidly-changing line, so a screen reader announces every change (opencode: 40 announcements in one turn). Suggest `aria-hidden`/`aria-live="off"` on the streaming line, keeping the status role for the "Working" label.
- **Reduced motion**: the sweep honours `prefers-reduced-motion`, but the pulsing dot uses Tailwind `animate-pulse`, which does not.

### Both bar findings closed (96bdcd5d6) — verified

- Streaming line now carries `aria-hidden` with a WHY comment; `role="status"` stays on the container so the "Working" state is announced once while the 40×-per-turn line churn is silent. Verified: `aria-hidden` sits on the `chat-stream-preview-line` span, not the container.
- `.ct-stream-dot { animation: none; }` added inside the `prefers-reduced-motion` block, so the dot stops alongside the sweep. Verified in source.
- Live confirmation turn after the commit (build 284d3303d): codex / opencode / claude all 0 jank / 0 dupes / 0 leftovers, leads +98…+20619 ms, zero notifications on working turns.

**No open items remain in the matcher/display lane.**

### Bar findings closed + decorations stripped — final state verified

- `96bdcd5d6`: `aria-hidden` moved onto the `chat-stream-preview-line` span (not the container), so `role="status"` still announces the "Working" state once while the 40×-per-turn line churn is silent; `.ct-stream-dot { animation: none; }` added under `prefers-reduced-motion`. Both verified in source.
- Live confirmation after that commit (build 284d3303d): codex / opencode / claude all 0 jank / 0 dupes / 0 leftovers, leads +98…+20619 ms, zero notifications on working turns.
- `5b25312b7` (decoration strip per operator): diff is **markup/CSS removal only** — the sweep track, its keyframes and reduced-motion variant, and the left accent border. No change to `show`, `lastLine`, `BAR_HEIGHT`, the collapse/unmount path, or `aria-hidden`; all five bar criteria therefore still hold, and the sweep criterion is retired by the operator's own decision. Trivial leftover: the inner div keeps a now-vestigial `relative` class (the absolute sweep it anchored is gone).

**Final signed state: branch HEAD 5b25312b7, shipping-accurate. No open items in the matcher/display lane.**
