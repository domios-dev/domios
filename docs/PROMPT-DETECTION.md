# Prompt detection

Prompt detection architecture, state, structural fingerprints, and verification.

## Prompt Detection Architecture

The permission prompt detection pipeline detects when AI agents show interactive prompts (e.g., "Do you want to proceed? 1. Yes 2. No") and renders clickable approval buttons in the chat UI.

### Pipeline

1. **PTY output arrives** -- `OutputBuffer` appends raw bytes and feeds them into a per-tab VT100 parser (`vt100` crate).
2. **`process_pty_output()`** (`src-tauri/src/service/output.rs`) calls `has_event_markers()` on the cleaned chunk text. This is a cheap string check that gates the expensive path.
3. **If markers found** -- `get_screen_text()` on `OutputBuffer` returns the actual terminal screen content (what the user sees), correctly handling cursor positioning, alternate screen, and TUI overlays.
4. **`PromptDetector.evaluate_pty_output()`** (`src-tauri/src/agent/prompt.rs`) runs the state machine. It delegates to `parse_permission_prompt()` for structural fingerprint detection.
5. **If prompt found** -- returns a `PromptDecision::NewPrompt` or `PromptDecision::PromptUpdated`. `apply_prompt_decision()` sets `pending_prompt` on the Tab and emits `mux-state-changed`.
6. **Frontend** (`TabSessionChat.tsx`) reads mux state, sees the pending prompt, and renders response controls. User responds, which calls `mux_respond_prompt()` and sends the chosen keystroke to the PTY.

### Key source files

| File | Role |
|------|------|
| `src-tauri/src/agent/prompt.rs` | `PromptDetector` state machine (Idle -> Active -> Cooldown -> Idle) |
| `src-tauri/src/agent/detector.rs` | Structural fingerprint detection (`detect_interactive_prompt`, `parse_permission_prompt`) + event detection (`detect_claude_events`) |
| `src-tauri/src/pty/manager.rs` | `OutputBuffer` with per-tab VT100 parser, `get_screen_text()` |
| `src-tauri/src/service/output.rs` | `process_pty_output()` orchestration + `apply_prompt_decision()` |
| `src/components/Terminal/TabSessionChat.tsx` | Frontend prompt state + response controls |

### State machine: PromptDetector

Each tab has independent state:

```
Idle ──[prompt detected]──> Active ──[user responds]──> Cooldown ──[500ms]──> Idle
                               │                                                │
                               ├──[same options]──> (no-op)                     │
                               └──[different options]──> PromptUpdated          │
                                                                                │
Cooldown ──[hook arrives]──> Active  (hooks bypass cooldown)                    │
```

- **Idle**: No active prompt. Output is parsed for prompts.
- **Active**: Prompt is shown to the user. Same-options output is suppressed. Changed options trigger `PromptUpdated`.
- **Cooldown**: User responded (500ms). PTY auto-detection is suppressed to avoid re-detecting the same prompt before the agent clears it. Hooks (API path) bypass cooldown since they are authoritative.

### Structural fingerprint detection (false positive prevention)

Detection uses structural patterns, never hardcoded question text. This makes it agent-agnostic and resilient to wording changes.

**Requirements for a positive match:**
- 2+ numbered options in `N. label` format (e.g., `1. Yes`, `2. No`)
- PLUS at least one of:
  - A `>` or `❯` selector prefix on one of the options
  - A keyboard hint footer containing `Esc`/`Enter` with `cancel`/`amend`/`confirm`
- Options and footer/selector must be within **8 lines** proximity (prevents matching a numbered list in the answer that sits far above the actual prompt)
- Only the last **20 lines** of screen content are scanned (prompt is always at the bottom)

**What does NOT match:**
- Plain numbered lists (no selector or footer)
- Source code containing prompt-like strings (wrong structure)
- Numbered log timestamps
- Markdown numbered lists in agent answers

### Idle detection and status tracking

The status tracker (`agent_tracker`) determines whether an agent is Working, Waiting, or Idle:

- Uses a **screen content hash** (SHA-256 of `get_screen_text()`) instead of raw PTY bytes
- Cursor blinks and TUI redraws (ANSI noise) do not change the screen content hash, so they do not reset the idle timer
- Default idle timeout: **5 seconds** of unchanged screen content

### Two detection paths

| Path | Entry point | When used |
|------|-------------|-----------|
| **PTY auto-detect** | `process_pty_output()` -> `PromptDetector.evaluate_pty_output()` | Always runs for agent Tabs; reliable fallback |
| **Hook (API)** | `POST /api/tabs/{id}/hook/permission` -> `PromptDetector.hook_detected_explicit()` | Claude Code hooks call this; faster, bypasses cooldown |

Both paths converge on the same `PromptDetector` state machine and `apply_prompt_decision()`.

### Testing prompt detection

Unit tests cover the full matrix:

```bash
# PromptDetector state machine (14 tests)
scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib prompt::tests

# Structural fingerprint detection (30+ tests)
scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib detector::tests
```

Key test categories in `detector.rs`:
- **True positives**: real Claude prompts (bash, file write, read), generic TUI menus
- **False positives**: numbered answers, bug analyses, step instructions, code reviews, phased plans, comparison lists, security audits
- **Edge cases**: prompt buried after long output, old prompt scrolled off, question mark before plain list, `Esc` text far from list

E2E tests (`test_hooks.py`) verify the full lifecycle: hook -> pending_prompt set -> respond -> prompt cleared.

UI tests (`prompt-approval.js`) verify button rendering and click behavior.

## What's NOT tested

- Clipboard/image paste (needs OS-level interaction)
- Cross-platform PTY differences (PowerShell vs bash)
- Session restore with active Claude across restart
- Real Claude permission prompt → chat UI → approve → Claude continues (needs real Claude running)

## Reviewer's checklist

Before approving a PR, use **`docs/contributing/review-checklist.md`** to verify evidence discipline, test methodology, documentation updates, and code quality.

<!-- revision: 1 -->
