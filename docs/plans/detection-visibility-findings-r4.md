# Detection visibility — round 4: transition-latency + live Stop/Esc

Source of truth: `docs/plans/detection-rebuild-deepening-loop.md` (rev 5). Prior: r1–r3.
dev daemon `3f477246`; classifier has uncommitted R3/R4 edits (mtime 18:37). For this
round I rebuilt `render_timeline` against the WIP classifier and **regenerated all 46
casts** to a scratch dir, so every number below reflects the CURRENT (edited) classifier.

## 1. Working→Idle transition-latency table — `testbench/transition-latency.py`

Per transition: the quiescence the classifier saw WHEN it declared idle
(`semantic_idle_ms` = detection latency), and the inferred idle branch
(`ready_prompt/composer` vs `safety-net`; safety-net requires both clocks
≥ STUCK_WORKING_IDLE_MS=5000ms, so any idle below that came from the composer anchor).

```
python3 testbench/transition-latency.py <regen-dir>          # fresh renders
python3 testbench/transition-latency.py --replays            # committed (may be STALE)
```

**Per-provider over 164 transitions (current classifier, all 46 casts):**

| provider | transitions | ready_prompt/composer | safety-net | sem_idle med (ms) | sem_idle max |
|---|---|---|---|---|---|
| claude   | 85 | 85 | 0  | **9**    | 73 |
| codex    | 55 | 55 | 0  | **44**   | 72 |
| cursor   | 12 | 0  | 12 | **5053** | 5080 |
| opencode | 12 | 0  | 12 | **5044** | 5070 |

### The R3/R4 latency gap, localized
- **codex & claude**: working→idle resolves via `ready_prompt()` (composer anchor
  `❯`/`›`) in **~9–44ms median** — prompt and correct. The 5000ms safety-net never fires.
- **cursor & opencode**: **100% of transitions fall through to the 5000ms safety-net**
  (~5.05s median). `ready_prompt()` does not recognize their composer glyph, so idle is
  only declared when both quiescence clocks cross 5000ms.

⇒ **Actionable for Loop Lead:** add cursor + opencode composer anchors to
`ready_prompt()` (`offline_state_classifier.rs`). That moves their working→idle
detection from ~5s to ~tens of ms and removes the safety-net dependence (the safety
net should be a backstop, not the primary idle path for two of four providers).
Re-run `transition-latency.py` after — target: safety-net count → 0 for cursor/opencode.

## 2. Live Stop / Cancel / manual-Esc → Idle (disposable dev panes)

Created disposable dev workspace `d34a10c7`, spawned claude/opencode/codex agents,
drove each to Working (essay prompt, confirmed `agent_state==working`), issued the
action, polled `/api/panes` to idle. **Workspace closed/cleaned after.** Pure `/api/*`,
macOS, no DOM.

| provider | Stop (`/stop`) → idle | manual Esc (`/send key esc`) → idle |
|---|---|---|
| claude   | **97ms**  | 263ms |
| codex    | **87ms**  | 289ms |
| opencode | **74ms**  | **7160ms** |

- **Stop is fast and correct on all three** (sub-100ms) — `/stop` writes the provider
  `stop_keystroke()` via `write_raw`.
- **opencode manual-Esc → idle = 7.16s** — lands in safety-net territory, the SAME
  composer-anchor gap as §1: raw ESC stops the turn but leaves opencode at a composer
  `ready_prompt()` can't match, so idle only fires via the 5s safety-net. (claude/codex
  Esc resolve in ~270ms via their composer anchor.) Fixing §1 also fixes this.
- The "trust this folder" claude startup prompt was correctly classified **Question**
  (validates the Question path live).

Not yet run live: cursor Stop (Ctrl+C) and `cancel` mode (`/prompt/clear`, needs a
pending question surface). Harness ready: `testbench/cancel-oracle.sh --pane <id> --mode stop|esc|cancel`.

### Tool fix this round
`cancel-oracle.sh` esc payload corrected: SendAction needs the serde tag —
`{"actions":[{"type":"key","key":"esc"}]}` (was missing `"type"`; the API rejects
`{"key":...}` with `missing field 'type'`).

## 3. Staleness (carried from r3, re-confirmed)
All 46 committed `timeline.json` predate the 18:37 classifier edit → STALE. This round
I regenerated to a scratch dir (not the tree) for measurement. Before any committed
gate run after a Rust change: `testbench/acceptance-suite.sh --regen`.

## 4. DOM (unchanged)
Codex ChatUI DOM order remains **manual / Windows-only** (Playwright CDP→WebView2; macOS
WKWebView has no attach). Data-layer ordering gate (`chat-order-gate.py`) stays GREEN.

## Tools added/updated
- `testbench/transition-latency.py` — per-provider/turn working→idle latency + idle-branch attribution.
- `testbench/cancel-oracle.sh` — esc payload fixed; Stop/Esc validated live on 3 providers.

<!-- revision: 1 -->
