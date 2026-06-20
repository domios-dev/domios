# Facet 7 — OSC 133/633 shell-integration → chat UI (strategy/design)

> Owner: Dante (orchestrator), from Andrey via ROG 2026-06-20. Target = the CHAT UI:
> surface command boundaries / status / cwd / decorations in chat (VS Code style),
> deterministically. Mandate: clean architecture, NO band-aids, NO hardcoded patches,
> NO legacy cruft — refactor what needs refactoring. I own the design + delegate the
> build. Branch `plugins-rethink`.

## Empirical grounding (measured, not assumed)
The real RT3 captures (`tests/fixtures/agent-state/{claude,codex}/*/recording.cast`)
contain ZERO OSC 133/633/7 — only OSC 0 (window title). **Agents do not self-mark turn
boundaries.** This fixes the layering and kills the conflation risk:
- **OSC 133/633 = the SHELL layer.** After we INJECT a shell-integration script, the
  shell emits semantic markers: command start/end, exit code, command text, per-command
  cwd, and clean output segmentation — for shell panes AND for the shell command that
  launches an agent (its start/exit).
- **Agent TUI internal state (working / question / done) = the vt100 SignalExtractor +
  classifier** (P1/P2). OSC 633 does NOT and CANNOT replace it (agents emit no markers).
- Therefore 633 (a) replaces `detector.rs`'s brittle last-20-lines prompt heuristic FOR
  SHELL PANES, (b) gives reliable cwd (no OSC 7 race), (c) powers chat command
  decorations. It COMPLEMENTS the agent-state classifier; the two are sibling signal
  sources, not competitors.

## Architecture — one pipeline, two signal sources (deterministic + structural)
```
PTY bytes
  -> [CORE] extraction (provider-agnostic, over the same byte stream):
       (a) SignalExtractor (P1a, vt100 grid)      -> StreamSignals      (agent TUI state)
       (b) SemanticMarkerParser (NEW, facet7)     -> SemanticEvents     (OSC 133/633/7)
            PromptStart(A) | PromptEnd(B) | CmdStart(C) | CmdEnd(D,exit)
            | CmdLine(E,text,nonce) | Prop(P, Cwd=...) ; ST or BEL ; 133 subset accepted
  -> [UNIFY] PaneAttentionState contract (facet5) consumes BOTH:
       shell panes  -> command lifecycle events drive chat command items + decorations
       agent panes  -> classifier state (P2) for turn state; shell layer adds launch/exit/cwd
  -> [CHAT UI] ChatView renders shell-command items: command text, status
       (running / ok / failed + exit code), cwd, the output segment (between C and D),
       with rerun / jump-to-command / copy affordances (VS Code command-decoration UX).
```
The SemanticMarkerParser sits BESIDE the SignalExtractor in the same core extraction
layer — the "core extracts generic signals, consumers interpret" model (register +
consume). Core never branches on a provider id.

## Components
1. **SemanticMarkerParser (core, Rust).** Streaming parser pulling OSC 133/633/7 out of
   the PTY byte stream into typed `SemanticEvent`s. Handles ST (`\x1b\\`) and BEL (`\x07`)
   terminators, partial sequences across chunk boundaries, the OSC 133 A/B/C/D compatible
   subset, and the 633;E nonce guard (command text must carry the per-session nonce, else
   it is untrusted output spoofing). Pure over the stream; deterministic.
2. **Shell-integration injection (per-shell scripts).** Extend `build_session` /
   `shell_integration.rs` to inject the integration script per shell (bash
   PROMPT_COMMAND/PS0, zsh precmd+preexec, pwsh PSConsoleHostReadLine wrap, fish events)
   emitting A/B/C/D/E/P with the nonce. We already inject env + a pwsh prompt fn there —
   that is the hook. (ROG pulling the exact VS Code scripts as the reference.)
3. **Unified PaneEvent plumbing.** The backend reader (`pty_lifecycle`) feeds
   SemanticEvents into the SAME event channel that carries agent-state, into one
   `PaneAttentionState`/event contract — not a parallel ad-hoc path.
4. **Chat UI command items + decorations.** chat-core ingests command-lifecycle events as
   a first-class "shell command" chat item (distinct from agent messages); ChatView
   renders status/exit/cwd + rerun/jump/copy.
5. **Retire the shell heuristic (parity-gated).** Where markers are present,
   `detector.rs`'s prompt-scan for shell panes is replaced by deterministic events; keep
   it ONLY as the no-markers fallback. Gate the switch behind a parity test (markers vs
   heuristic agree on a real corpus) BEFORE deleting any heuristic path.

## Integration with the agent-state phases (not a bolt-on)
- **P1:** SemanticMarkerParser ships in the same core extraction step as SignalExtractor;
  SemanticEvents become a field consumable alongside StreamSignals.
- **P5 (UI + streaming):** the chat command-item rendering + decorations ARE the shell
  half of the unified PaneAttentionState → chat UI. One contract, two sources.
- detector.rs consolidation (P3) gains a deterministic replacement for the shell case.

## Tasks (I delegate; one Opus per track, worktree-isolated)
- **OSC1 (core, Rust):** SemanticMarkerParser — OSC 133/633/7 → typed SemanticEvents;
  ST+BEL, partial-chunk carry, 133 subset, nonce guard. TDD against synthetic fixtures.
  Provider-agnostic; additive (not yet wired to a consumer). NO new parser for OSC if an
  existing OSC handler exists — check first.
- **OSC2 (Rust, shells):** per-shell injection scripts in shell_integration.rs +
  build_session wiring (bash/zsh/pwsh/fish), nonce per session. Verify by capturing a
  real shell session (RT1 `.cast`) and asserting A/B/C/D/E/P appear. Seeds from ROG's
  VS Code reference. ROG owns Windows pwsh verification.
- **OSC3 (Rust):** plumb SemanticEvents from the backend reader into the unified PaneEvent
  contract + emit to chat-core (same channel as agent-state). No parallel path.
- **OSC4 (TS, chat UI):** shell-command chat item + decorations (status/exit/cwd/output
  segment) + rerun/jump/copy in ChatView/chat-core. Clean component, follows chat-core
  patterns.
- **OSC5 (Rust):** parity-gate (markers vs detector.rs heuristic on a real corpus), then
  retire the shell-pane heuristic; keep markerless fallback. CI grep gate: no provider-id
  literal leaks into the parser.
- **OSC6 (eval + cross-platform):** capture real shell sessions per OS, verify command
  segmentation + cwd; ROG Windows pwsh + Playwright chat-decoration oracle (Layer B).

## Sequencing
OSC1 (parser) + OSC2 (injection) in parallel (both off the OSC spec) → OSC3 (plumbing) →
OSC4 (chat UI) → OSC5 (retire heuristic, parity-gated) → OSC6 (eval/cross-platform).
OSC1 starts now (needs only the spec). OSC2 starts when ROG's VS Code scripts land.

## Guardrails
- Nonce-guard 633;E (untrusted output can spoof command text otherwise).
- Accept ST and BEL terminators; accept the OSC 133 A/B/C/D subset (interop with tools
  emitting 133).
- cwd via 633;P;Cwd (per-command) preferred over OSC 7 (race-free).
- No per-provider hardcoding in the parser; deterministic + pure over the stream.
- Parity BEFORE deletion: never remove a heuristic path until markers are proven equal on
  a real corpus.
- Cross-platform: all four shells; Windows pwsh PSReadLine path verified by ROG.

<!-- revision: 1 -->
