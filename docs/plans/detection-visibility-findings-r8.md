# Detection visibility — round 8: after Lead's composer fix (before/after)

Source of truth: `docs/plans/detection-rebuild-deepening-loop.md` (rev 5). Prior: r1–r7.
Lead change: `composer_cursor_ready` dropped the `alt_screen` requirement (now
3-arg `cursor_visible && cursor_row + 8 >= rows`). Full regen 49/49 against it.

## Latency gate — BEFORE (R9) → AFTER (Lead composer fix)
| provider | before safety-net | after safety-net | after sem_med | after sem_max |
|---|---|---|---|---|
| claude | 0/86 | 0/86 | 10ms | 73ms |
| codex | 0/55 | 0/55 | 44ms | 72ms |
| cursor | 0/44 | 0/44 | 27ms | 69ms |
| opencode | **4/19** | **0/26** | **1537ms** | 1565ms |

`python3 testbench/transition-latency.py <regen> --gate` → **GATE GREEN** (all 0 safety-net).
opencode safety-net FIXED. Caveat: opencode idle latency is now ~1537ms (just past the
1500ms WORKING_GRACE — a settling delay) vs ~30ms for the others; 50× better than the old
5000ms safety-net but not snappy. idle-matrix ambiguous frames went **160 working / 577
idle → 60 working / 677 idle** (working↔idle ambiguity much reduced).

## Strobe — BEFORE → AFTER
| scope | before strobes | after strobes |
|---|---|---|
| opencode (all) | 44 | **43** |
| full corpus | 156 | **155** |

The composer fix barely moved strobes because the dominant opencode strobing is a
**separate defect**: `opencode systems_design` has **41 strobes, ALL question↔working**
(24 question-between-working + 17 working-between-question) — opencode mid-generation
frames intermittently flip to Question. NOT idle-related, untouched by the composer fix.
Open item for Lead.

## Permission false-idle guard — FAIL (required fixture)
`testbench/permission-guard-check.sh` →
`permission_box_visible_cursor_v1: states={'idle':121} → RESULT: FAIL (FALSE-IDLE — all 121 frames idle)`

A numbered-choice permission box with a VISIBLE cursor in the bottom region
(alt_screen=false) is classified 100% idle. Cause: relaxed `composer_ready` fires on the
visible bottom cursor; `classify_terminal_menu_question` is gated on `cursor_hidden`
(false here) so it's skipped; no picker → idle wins over question. Fixture kept as the
required failing→passing regression test. Fix: guard `composer_ready` against an on-screen
choice/option box (numbered menu + footer) even with a visible cursor.

## OpenCode HTTP-abort — not available in current architecture
- Launch = plain TUI `opencode -m <model>` (dev log `build_agent_command`); **no `--port`,
  no listening socket** (only outbound HTTPS to the model). No HTTP control plane.
- CodeTerm reads opencode chat from its **on-disk SQLite DB** (`read_structured_chat` →
  `host.fs.sqliteQuery`) + per-pane `structured_poller`; the binding plugin PUSHES hooks
  to CodeTerm. Direction is opencode→CodeTerm; CodeTerm holds no opencode base URL.
- Session id known: `ses_…`. For abort, Lead must run `opencode serve --port <P>` then
  `POST /session/{id}/abort`. `stop-oracle.sh --mode http-abort --oc-base --oc-session`
  is ready. Until then **opencode /stop is UNRESOLVED** (raw ESC doesn't halt; r6).

## Standing tools / fixtures
- `testbench/transition-latency.py --gate`, `strobe-metric.py`, `stop-oracle.sh`
  (+`--mode http-abort`), `permission-guard-check.sh`, `chat-order-gate.py`.
- Fixtures: `testbench/replays/synthetic/permission_box_visible_cursor_v1`,
  `testbench/fixtures/codex-chat-order-2bace0d6.json`.
- `testbench/diagnostics/opencode_ambiguity/` bundle for Lead.
- rev15 acceptance-suite wiring: `docs/plans/detection-visibility-after-fix-runbook.md`.

## Open items (all Lead-side; I rerun the gates after each)
1. opencode question↔working strobes (41 in systems_design) — separate from composer.
2. permission-box false-idle guard.
3. opencode interrupt via server-mode/HTTP-abort.
4. opencode idle latency ~1.5s (settling) — minor.

<!-- revision: 1 -->
