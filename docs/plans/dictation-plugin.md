# Dictation plugin — spec & contract

**Status:** design converged (BLACKSTAR GA ↔ ROG GA, brainstorm). Target branch: `feat/architecture_update`.
**Decision:** ship as a **Native** plugin now (tier A). The two surfaces it needs are tier-agnostic and become the contract a future **Process**-tier host must satisfy (tier B). Do not build the Process host and the dictation UX in parallel — let dictation drive the input-buffer spec first.

## Why native-now (not process-first)

- `PaneFocused` is already emitted on this branch (`api/events.rs:248`, `service/mod.rs:156`) → real focus targeting for free; removes the #123 unknown.
- The Process host is declaration+registration only (`activate()` falls through to the trait default — no spawn, no JSON-RPC stdio, no health). Writing it from zero **while** shipping dictation UX = two big bets at once.
- Both surfaces below are identical whether dictation is Native (now) or Process (later). Spec them once.

## Invariant (holds for every tier)

**The STT engine stays external, behind the `/transcribe` URL contract** (see `codeterm docs TRANSCRIBE`). Never compile a whisper model into the binary. The plugin owns capture + hotkey + the UX; the engine is a swappable URL (local whisper, remote peer, cloud).

## Prerequisite fixes (must land BEFORE the plugin can work)

- [ ] **Wire `dispatch_event` to the event bus.** `PluginManager::dispatch_event` (`plugin/mod.rs:411`) is correct but has no caller in production: in `lib.rs` the `event_broadcaster` only has the `ContributionsChanged → frontend` subscriber. `on_event` on every active native plugin is currently dead code. Add a subscriber task: `while let Ok(ev) = sub.recv() { plugin_manager.dispatch_event(&ev); }`. Small, but blocks all event-driven plugins.
- [ ] **Add a managed-input marker to the pane model.** `PaneType` is `Terminal | Browser(legacy)`; the "does this pane have a managed input box" distinction is today only a runtime heuristic (`screen_has_agent_tui()` pattern-matching PTY screen text). The input-buffer API needs a structural flag — a `PaneInputMode` enum (or bool) on the pane struct — not a screen-text guess.

## Surface 1 — pane input-buffer API (the reusable prize; NOT dictation-specific)

One seam any external producer streams text into a pane through — dictation, live translation, snippet/AI autocomplete. This is the real "convenient external integration" payoff.

```
input.begin(pane_id) -> session_id     # open an UNCOMMITTED region in the pane input; pane state -> Recording
input.partial(session_id, text)        # replace the uncommitted region (idempotent; re-render as partials arrive)
input.commit(session_id, { submit })   # finalize region into the input; if submit, send; clear state
input.cancel(session_id)               # drop the uncommitted region; clear state
```

Pane state surfaced to chrome (border pulse / status glyph): `Idle | Recording | Transcribing | Committing`.

**Managed-input vs raw PTY (degrade rule):**
- Managed-input panes (agent prompt box) → full API, live partials.
- Raw PTY panes (no managed input) → **commit-only**: no live partials (partial-replace would mean erase+rewrite against an arbitrary TUI). `begin`+`commit` still work; `partial` is a no-op or buffers until commit.
- Requires the managed-input marker prereq above.

## Surface 2 — trigger event

```rust
SystemEvent::DictationRequested { pane_id: Option<String> }
//  None  = "focused pane at time of hotkey" — resolved by the plugin's on_event
//          handler from the last PaneFocused it received (PaneFocused is live).
//          Avoids a hotkey-vs-focus-change race.
```

- Emitted by **(a)** a native global hotkey (`tauri` global-shortcut, cross-platform) and **(b)** a UI button.
- Routing is already handled: `dispatch_event` filters active plugins by their `capabilities.hooks` — once the bus subscriber prereq lands, the plugin receives it via `on_event`. The only NEW work is emitting the event + wiring the button/hotkey.

## The plugin (native, end to end)

```
hotkey / UI button
   └─> emit SystemEvent::DictationRequested { pane_id: None }
          └─> dictation plugin on_event:
                 resolve target = pane_id.or(last PaneFocused)
                 input.begin(target)            # state -> Recording
                 capture mic (PTT toggle)
                 state -> Transcribing
                 POST {audio} -> {transcribe_url}/transcribe   # external, swappable
                 (stream: input.partial(sess, text) as segments arrive)
                 input.commit(sess, { submit: cfg.auto_submit })
```

Config `[integrations.voice_input]` (or plugin settings): `transcribe_url` (defaults to `voice_transcribe.daemon_url`), `hotkey`, `device`, `language`, `auto_submit`.

## Streaming caveat (unchanged)

Live `input.partial` needs the STT side to emit partial/segment results. The reference whisper daemon is one-shot today; until it streams, the plugin commits once at the end (no live fill). Streaming is an STT-side enhancement, independent of this contract.

## Migration to Process tier (B)

When the Process host lands (spawn + JSON-RPC stdio + health/restart), the dictation plugin moves out of core unchanged: Surfaces 1 & 2 become JSON-RPC methods/events the host bridges. This doc's API IS the contract the host must satisfy.
