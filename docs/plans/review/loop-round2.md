# Review loop — round 2 findings

## R2-B (full-diff sweep, codex) — DONE
HIGH:
- service/mod.rs:179 emit_state_changed — snapshots state BEFORE the 50ms coalesce window + suppresses
  later emits while mux_emit_pending; FE now consumes payloads + generation-guards, so fast follow-up
  mutations (restore/chatBackend/session-id/backing) can be LOST until an unrelated later emit. Fix:
  snapshot AFTER the sleep, or dirty-flag loop until quiet. Test: 2 mutations in one window -> 2nd emitted.
MED:
- api/pane_routes.rs:57 /api/panes accepts any chat_backend id WITHOUT spawnable() validation (agents/spawn
  validates; /api/panes doesn't) -> 200 then tab vanishes async. Fix: validate via registry+spawnable, 400.
- provider-cursor plugin.ts:144 detectSessionId binds any UUID child even with NO meta.json/store.db; ts=0
  bypasses maxAge -> empty leftover dir binds a non-session. Fix: require meta.json|store.db; real ts only.
- reducer.ts:92 SET_STATE ignores action.state.focused_pane_id (snapshots carry it) -> restore/hydrate can
  leave focus null/stale under payload-based flow. Fix: copy focused_pane_id when it names a live pane.
  [SAME CLASS as the recently_closed reducer-drop already fixed.]
LOW:
- transport.rs:230 spawn_pane_atomic_with: no rollback on launch Err -> blank focused tab (close on Err).
- provider-cursor plugin.ts:353 POSIX trustPrefix writes raw cwd into JSON -> quote/backslash breaks it
  (ineffective trust marker). Fix: JSON-escape workspacePath.
CLEAN: ProviderRegistry facade capability-based; classify_spawn_target shared; chatBackend openSession
failure closes tab; cursor --resume/trust-gate/JSONL/model-fallback/ambiguity-refusal covered; routes auth-gated.

## R2-A (adversarial on fixes, codex) — DONE — fixes largely CLEAN
CONFIRMED CLEAN: closed peek-paths, PaneReopenGuards serialization, hot_reload tick lock, spawn_blocking
closures, opencode/generic refuse-ambiguous (no off-by-one), cursor trust-gating, CRLF normalize, claude split.
NEW:
- MED pane_transport_chat.rs:264 chatbackend_reopen_if_restored TOCTOU: after openSession (no mux held) it
  unconditionally migrates+spawns poll-loop; if pane closed/backing-changed in between, new session leaks +
  poll-loop on dead pane. Fix: re-acquire mux, verify pane still ChatBackend{old_session}+restored before
  migrate/spawn; else closeSession(new)+return.
- MED provider-cursor plugin.ts:138 detectSessionId empty-UUID false positive [DUP of R2-B] -> refuses legit
  single candidate. Fix: candidate must satisfy sessionExists contract (meta.json|store.db).
- LOW closed/store.rs:412 delete_pane_row/delete_group/delete_pane swallow sqlite failures; reopen returns
  success after materialize -> on delete failure entry stays -> retry duplicates. Fix: delete returns Result+validate.

## Round-2 combined triage -> fixes
- W1 (state-delivery): HIGH emit_state_changed coalesce-drop (service/mod.rs) + MED reducer focused_pane_id drop. [dispatched]
- W3 (cursor): MED empty-UUID candidate + LOW POSIX trust JSON-escape (provider-cursor plugin.ts). [dispatched]
- W2 (restore/spawn Rust, HELD until daemon-CLI lands): MED chatbackend_reopen TOCTOU + MED /api/panes spawnable
  validation + LOW transport.rs launch-Err rollback + LOW closed/store delete Result.
