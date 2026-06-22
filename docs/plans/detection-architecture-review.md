# Detection / provider architecture review — prioritized findings

> Loop Lead review (2026-06-21), scope: PTY parsing + 3-state classification for claude/codex/
> opencode/cursor, replay gates, false Working/Question risks, capability/plugin boundary.
> EXCLUDES auto-approve/blocker policy (owned separately). Baseline `3f477246` + current WIP.

## What's healthy (verified)
- **Core classifier is provider-agnostic.** `offline_state_classifier.rs` + `service/output.rs` have
  ZERO provider-id string literals (grep clean). `classify_state` branches on structure
  (status widget / active-turn footer / ready-prompt glyph / composer cursor / option-block /
  cursor-hidden), never on a provider id.
- **One stateful oracle.** `render_timeline` drives the SAME `classify_state` via `classify_signals`
  (picker gated on `picker_active`, `Motion::Still` to match live). Replay == live.
- **Rich replay gates** (testbench): `transition-latency.py --gate`, `strobe-metric.py`,
  `idle_hold_gate.py`, `idle_latency_oracle.py`, `opencode-permission-gate.sh` (C9),
  `opencode-server-check.sh`, `stop-oracle.sh`, repaint proof. transition-latency is GREEN
  (0 safety-net) for all 4 providers.
- **Centralization is capability-gated** (`needs_structured_prompt_server`), no provider-id branch,
  wired into every spawn path via one `ensure_for_spawn` helper.

## P1 — correctness / latent breakage
1. **Unverified opencode QUESTION reply route.** `opencode_sse::reply_endpoint` for
   `PromptKind::Permission` was just fixed to `/session/{}/permissions/{}` (the `/api/.../reply` form
   404'd). `PromptKind::Question` STILL uses `/api/session/{}/question/{}/reply` — almost certainly
   the same stale shape → answering an opencode QUESTION (not permission) will 404. **Add:** verify
   against the opencode SDK (`postSessionIdQuestions…`), fix the route, add an `opencode_sse` unit
   test for the question reply_endpoint, and extend the C9 gate to drive a QUESTION (not only a
   permission). File: `src-tauri/src/service/structured_prompt/opencode_sse.rs`.
2. **Provider-specific parsing in the shared classifier.** `extract_opencode_status_line` /
   `is_opencode_chrome_row` / `strip_opencode_status_prefix` are opencode-tuned structural rules
   living in core `offline_state_classifier.rs`, not behind a provider capability/plugin. Works, but
   it's a soft capability-leak: opencode chrome changes break it silently and other providers can't
   plug their own status widget. **Add:** either a provider `parse_status_line` capability hook
   (preferred long-term) or, minimally, a per-provider "working → status_line extracted" replay
   fixture so a chrome change fails a gate. Files: `offline_state_classifier.rs` (extractors),
   `docs/contributing/architecture.md` (boundary note).
3. **`confirm_style_footer` (Round 11) false-Question surface.** numbered/checkbox options + an
   enter/esc/nav key-hint footer ⇒ Question regardless of `cursor_hidden`. The current corpus shows
   0 negative FPs (with/without diff), but assistant PROSE that quotes a key-hint footer near a
   numbered list could false-Question. **Add:** a permanent negative replay (assistant prose with a
   numbered list + "press enter … esc to cancel" text) asserting it stays NOT question. File:
   `offline_state_classifier.rs` tests + a `detect-scenarios` negative.

## P2 — robustness / coverage
4. **Restored-pane reclassification gap.** Seeded Idle on restore (Invariant 1 satisfied: 22/22
   panes non-null), but the live-state poller only sweeps `agent_tracker.all_ids()`, so a restored
   pane that is actually mid-QUESTION (parked menu on the restored screen) shows idle until its first
   post-restart output. `recompute_agent_state` also bails on `quiescence()==None`. **Add:** poller
   sweeps mux agent panes (not only tracked ptys) + `recompute` treats missing quiescence as
   fully-quiescent so it classifies from the restored screen. Files: `agent/live_state.rs`,
   `service/output.rs` (`recompute_agent_state`).
5. **`composer_ready` visible-cursor idle.** Provider-agnostic (visible cursor in bottom input
   region), gated post-grace + options-block veto. Residual risk: a full-screen TUI QUESTION with a
   visible cursor and no recognized option-block markers could false-idle. **Add:** a negative replay
   with a visible-cursor question lacking veto markers.
6. **5s stuck-Working safety net is a timeout backstop.** Structural composer signals make it rarely
   fire (transition-latency GREEN), but it can still mask an unknown layout. **Keep** the
   transition-latency `--gate` (0 safety-net) wired as a HARD fail in `acceptance-suite.sh` for all
   providers; add `strobe-metric.py` + `idle_hold_gate.py` to the suite if not already enforced.

## P3 — capability/plugin parity
7. **Parity gaps for new capabilities.** `parity_test.rs` covers build_launch_command / readiness /
   task_delivery / detect_from_title / detect_from_output / session_file_path, but NOT the newer
   capability surface a JS drop-in can implement: `needs_structured_prompt_server`,
   `supports_structured_prompts`, `stop_keystroke`. Per architecture.md rule 2 (native+JS parity in
   lockstep), **add** parity cases for these so a drop-in can't silently drift. File:
   `src-tauri/src/plugin/js_runtime/parity_test.rs`.

## Suggested order
P1.1 (question reply route — real 404 bug) → P1.3 (confirm_footer negative lock) → P2.4 (restored
reclassify) → P3.7 (parity) → P1.2 (status-extractor capability) → P2.5/P2.6 (negatives + gate
wiring). All non-auto-approve.

<!-- revision: 1 -->
