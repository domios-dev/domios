# Spawn / Inject Readiness Gate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `agent spawn`, `send-text`, and any CLI/REST endpoint that injects text into a PTY return success **only after** the text was actually accepted by the receiver and submission was confirmed — not optimistically right after a write.

**Architecture:** Extend `ServiceContext::send_text_to_pty` with a verified variant `send_text_verified` that, after the bracket-paste write, samples the screen buffer to confirm (a) the text we sent appears as input, and (b) for `enter=true`, that submission landed (new shell prompt OR — for TUI agents — the input box drained / "Working" indicator appeared). Wire this verified variant into all three inject paths: orchestrator `launch_agent`, the `/api/panes/{id}/send` REST handler, and the hidden-spawn flow in `service::spawn`. The `agent spawn` REST handler awaits a oneshot completion signal from the background launch thread before returning, so CLI sees real outcome — pass with timing or fail with reason. Per-provider `verify_submit_strategy` lets claude/codex providers express their own "submitted" marker (TUI-specific) instead of relying on shell-prompt heuristics. Bonus: relax `guard_against_shell_mangle` to ignore matches inside fenced markdown blocks so legitimate documentation/repro text passes.

**Tech Stack:** Rust (axum + tokio + parking_lot), the existing `ScreenStateHandle` / `vt100`-based screen buffer, the existing `ReadinessStrategy` / `AgentProvider` trait, the existing `arm_prompt_signal` / `screen_has_shell_prompt` heuristics. Frontend untouched.

---

## File Structure

**Created:**
- `src-tauri/src/service/inject_verify.rs` — `VerifyOpts`, `InjectionResult`, `InjectionError`, helpers `wait_for_echo`, `wait_for_submit_shell`. ~250 lines incl. unit tests of the helpers against synthetic screens.

**Modified:**
- `src-tauri/src/service/mod.rs` — add `mod inject_verify;`, re-export public types.
- `src-tauri/src/service/pty_io.rs` — add `send_text_verified(...)` method on `ServiceContext`.
- `src-tauri/src/agent/provider.rs` — add `VerifySubmitStrategy` enum + new optional method `verify_submit_strategy() -> VerifySubmitStrategy` defaulting to `ShellPrompt` (so existing providers behave the same).
- `src-tauri/src/plugin/agent_claude/provider/mod.rs` — return `VerifySubmitStrategy::TuiInputDrained { idle: Duration::from_millis(800) }`.
- `src-tauri/src/plugin/agent_codex/provider.rs`, `src-tauri/src/plugin/agent_opencode/provider.rs` — same TUI strategy.
- `src-tauri/src/service/orchestrator.rs` — `launch_agent` to use `send_text_verified` and emit completion via a oneshot channel; `spawn_agent` accepts an optional `wait_ready` flag and awaits the channel before returning.
- `src-tauri/src/service/spawn.rs:649` — same verified-send for the hidden-spawn launch_cmd path.
- `src-tauri/src/api/agent_routes.rs` (or wherever `spawn_agent` REST handler lives — verify with grep) — pass `wait_ready=true` from REST, return verified result in the JSON body.
- `src-tauri/src/api/pane_routes.rs` — `/api/panes/{id}/send` body gains optional `verify: bool` (default `true` for CLI calls, `false` only for the legacy unsafe path); handler dispatches to `send_text_verified` when `true` and surfaces success/failure.
- `cli/src/main.rs` — `guard_against_shell_mangle` skips substrings inside ` ```...``` ` fenced code blocks; expose `--no-wait` flag on `agent spawn` and `send-text`.
- `cli/src/cmd/agent.rs`, `cli/src/cmd/send_text.rs` (or wherever the CLI subcommands live) — surface verified-result fields (`elapsed_ms`, `submitted`) in human + JSON output; non-zero exit on `InjectionError`.

**Tests:**
- `src-tauri/src/service/inject_verify.rs::tests` — pure logic tests (~12 cases).
- `src-tauri/src/service/pty_io.rs::tests` — extend with `send_text_verified` integration over a fake PTY/screen pair (~5 cases).
- `cli/src/main.rs::tests` — `guard_against_shell_mangle_ignores_fenced_code` (~4 cases).

---

## Pre-flight

- [ ] **Step 0.1: Confirm branch**

Run: `git rev-parse --abbrev-ref HEAD`
Expected: `chore/cli-dev-bin-and-mem-slim`

- [ ] **Step 0.2: Confirm dev daemon is the build under test**

Run: `curl -s http://127.0.0.1:7686/api/health | jq '{build,build_number}'`

If `build` ≠ `git rev-parse HEAD`, re-run `./build.sh dev` so the live daemon matches the source you're editing.

---

## Task 1: Failing repro for the optimistic-success bug

**Files:**
- Create: `src-tauri/src/service/inject_verify.rs`
- Test: `src-tauri/src/service/inject_verify.rs` (inline `#[cfg(test)] mod tests`)

The repro is a unit test against the **post-write screen check** logic, not a real PTY. We construct a synthetic screen buffer that contains a partial echo (the bug we observed: `codeterpunset CLAUDECODE…`) and assert that the verifier flags it as "echo missing/corrupted" rather than "echo OK".

- [ ] **Step 1.1: Create the new module file with stubs**

```rust
// src-tauri/src/service/inject_verify.rs
use std::time::Duration;

/// Configuration for a verified text injection.
#[derive(Debug, Clone)]
pub struct VerifyOpts {
    pub require_prompt_first: bool,
    pub verify_echo: bool,
    pub verify_submit: bool,
    pub prompt_timeout: Duration,
    pub echo_timeout: Duration,
    pub submit_timeout: Duration,
}

impl Default for VerifyOpts {
    fn default() -> Self {
        Self {
            require_prompt_first: true,
            verify_echo: true,
            verify_submit: true,
            prompt_timeout: Duration::from_secs(10),
            echo_timeout: Duration::from_secs(3),
            submit_timeout: Duration::from_secs(5),
        }
    }
}

#[derive(Debug, Clone)]
pub struct InjectionResult {
    pub bytes_written: usize,
    pub echo_observed: bool,
    pub submit_observed: bool,
    pub elapsed: Duration,
}

#[derive(Debug, Clone)]
pub enum InjectionError {
    PromptNeverReady { waited: Duration },
    EchoMismatch { expected_substr: String, screen_tail: String },
    SubmitNeverConfirmed { waited: Duration, screen_tail: String },
    PtyClosed { pty_id: String },
}

impl std::fmt::Display for InjectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PromptNeverReady { waited } =>
                write!(f, "shell prompt never appeared (waited {:?})", waited),
            Self::EchoMismatch { expected_substr, screen_tail } =>
                write!(f, "text not visible on screen after paste; expected {:?}, screen tail = {:?}",
                    expected_substr, screen_tail),
            Self::SubmitNeverConfirmed { waited, screen_tail } =>
                write!(f, "submission not confirmed (waited {:?}); screen tail = {:?}",
                    waited, screen_tail),
            Self::PtyClosed { pty_id } => write!(f, "pty {} closed mid-inject", pty_id),
        }
    }
}
impl std::error::Error for InjectionError {}

/// Returns true if `expected` (after normalising whitespace) appears as a
/// contiguous substring of `screen_tail`. Bracket-paste echoes show up
/// verbatim in zsh/bash; for TUI agents the text appears inside the input
/// box. Both cases are caught by simple substring search since the text
/// is typically distinct enough.
pub fn screen_contains_echo(screen_tail: &str, expected: &str) -> bool {
    if expected.is_empty() {
        return true;
    }
    // Strip excess whitespace from both sides — terminals reflow long input.
    let norm = |s: &str| -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let n_screen = norm(screen_tail);
    let n_expected = norm(expected);
    n_screen.contains(&n_expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_corruption_is_detected_as_missing_echo() {
        // The exact corruption from the bug report: prompt+text fused.
        let screen = "rollacode@host codeterpunset CLAUDECODE && claude --dangerously-skip-permissions \"$(cat /tmp/.ct-task-01219c41)\"41)\"";
        let expected = r#"unset CLAUDECODE && claude --dangerously-skip-permissions "$(cat /tmp/.ct-task-01219c41)""#;
        // The bug: the substring is NOT present verbatim because of the prompt fusion.
        // After normalising whitespace, the corrupted "codeterpunset" still doesn't match
        // "unset CLAUDECODE..." as a contiguous run.
        assert!(
            !screen_contains_echo(screen, expected),
            "verifier failed to detect corrupted echo — this is the bug"
        );
    }
}
```

- [ ] **Step 1.2: Wire the new module into the service tree**

Edit `src-tauri/src/service/mod.rs`. Add at the appropriate place (alphabetical or after another `mod xxx;`):

```rust
pub mod inject_verify;
```

And re-export the public types:

```rust
pub use inject_verify::{InjectionError, InjectionResult, VerifyOpts, screen_contains_echo};
```

- [ ] **Step 1.3: Run the test to confirm it passes (the repro is captured)**

Run: `./build.sh test 2>&1 | grep -E "inject_verify|test result" | tail -10`
Expected: the new test in `inject_verify::tests::observed_corruption_is_detected_as_missing_echo` is in the `915 passed` count (now `916 passed`); zero failures.

- [ ] **Step 1.4: Add the second repro: trailing-paren duplication**

```rust
#[test]
fn duplicated_trailing_paren_is_detected_as_corrupted_echo() {
    // The other corruption: the closing `)"` was duplicated → `)"41)"`.
    // This means the actual echo contains EXTRA chars beyond the expected text.
    // Echo presence by substring still passes here BUT the submitted command
    // is not the one we sent — we'll catch this via the submit-side check
    // in Task 5 (it produces a "command not found" line, not a fresh prompt).
    let screen = "rollacode@host codeterm % unset CLAUDECODE && claude --dangerously-skip-permissions \"$(cat /tmp/.ct-task-01219c41)\"41)\"";
    let expected = r#"unset CLAUDECODE && claude --dangerously-skip-permissions "$(cat /tmp/.ct-task-01219c41)""#;
    // Note: the substring IS present here (duplicate is appended), so echo-only check passes.
    // This is intentional: the duplicate-paren case is caught by submit verification, not echo.
    assert!(screen_contains_echo(screen, expected));
}
```

Run: `./build.sh test 2>&1 | grep -E "inject_verify|test result" | tail -10`
Expected: both tests pass.

- [ ] **Step 1.5: Commit**

```bash
git add src-tauri/src/service/inject_verify.rs src-tauri/src/service/mod.rs
git commit -m "test(service): add inject-verify scaffolding + repros for spawn race

Captures the two corruption patterns observed when 'codeterm agent spawn'
returns optimistically before zsh has consumed the bracket-paste:

- prompt-prefix fusion (\"codeterpunset\" instead of \"unset\" — caught by
  echo-substring check)
- duplicate trailing paren (caught later via submit verification, not echo)

No production-code wiring yet — those land in subsequent tasks."
```

---

## Task 2: Wait-for-echo helper backed by ScreenState

**Files:**
- Modify: `src-tauri/src/service/inject_verify.rs`
- Modify: `src-tauri/src/pty/screen_state.rs` — add `tail_text(pty_id, max_rows)` getter.

The helper polls the screen tail at ~50ms cadence until `screen_contains_echo` returns true OR `echo_timeout` elapses. The polling cadence is event-driven via the existing screen-update notify pattern; if that's not exposed, fall back to `tokio::time::sleep`.

- [ ] **Step 2.1: Add `tail_text` to ScreenState**

Edit `src-tauri/src/pty/screen_state.rs`. Find the `impl ScreenState { ... }` block. Add this method near the existing `arm_prompt_signal` (line 109 area):

```rust
/// Returns the visible text of the last `max_rows` rows of the screen
/// for `pty_id`, joined with `\n`. Returns `None` if no parser is bound.
pub fn tail_text(&self, pty_id: &str, max_rows: usize) -> Option<String> {
    let parser = self.parsers.get(pty_id)?;
    let screen = parser.screen();
    let (rows, cols) = screen.size();
    let start_row = rows.saturating_sub(max_rows as u16);
    let mut out = String::new();
    for row in start_row..rows {
        let row_text = screen.contents_between(row, 0, row, cols);
        out.push_str(&row_text);
        out.push('\n');
    }
    Some(out)
}
```

- [ ] **Step 2.2: Add `wait_for_echo` to inject_verify**

Edit `src-tauri/src/service/inject_verify.rs`. Append:

```rust
use crate::ScreenStateHandle;

/// Blocking wait until `expected` substring appears in the last `tail_rows`
/// rows of the screen, or `timeout` elapses. Polls at 40ms cadence.
/// Returns the final screen tail on success or timeout (so the caller can
/// include it in error reports).
pub fn wait_for_echo(
    screen_state: &ScreenStateHandle,
    pty_id: &str,
    expected: &str,
    timeout: Duration,
    tail_rows: usize,
) -> Result<String, String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let tail = screen_state
            .lock()
            .tail_text(pty_id, tail_rows)
            .unwrap_or_default();
        if screen_contains_echo(&tail, expected) {
            return Ok(tail);
        }
        if std::time::Instant::now() >= deadline {
            return Err(tail);
        }
        std::thread::sleep(Duration::from_millis(40));
    }
}
```

- [ ] **Step 2.3: Unit test wait_for_echo with a fake screen state**

Need to expose a way to construct a `ScreenStateHandle` for tests. Look at how existing tests in `pty/screen_state.rs` build one. If there's no public constructor, add `#[cfg(test)] pub fn new_for_tests()` to ScreenState and a way to push raw bytes. (Existing tests at the bottom of that file already do this — reuse those.)

```rust
// in inject_verify.rs::tests, after the existing tests:

#[test]
fn wait_for_echo_returns_immediately_when_text_already_present() {
    use crate::pty::screen_state::ScreenState;
    use parking_lot::Mutex;
    use std::sync::Arc;

    let state = Arc::new(Mutex::new(ScreenState::new()));
    {
        let mut s = state.lock();
        s.bind("pty-1", 24, 80);
        s.process("pty-1", b"rollacode@host codeterm % unset CLAUDECODE\n");
    }
    let result = wait_for_echo(
        &state,
        "pty-1",
        "unset CLAUDECODE",
        Duration::from_millis(200),
        5,
    );
    assert!(result.is_ok());
}

#[test]
fn wait_for_echo_times_out_when_text_never_arrives() {
    use crate::pty::screen_state::ScreenState;
    use parking_lot::Mutex;
    use std::sync::Arc;

    let state = Arc::new(Mutex::new(ScreenState::new()));
    {
        let mut s = state.lock();
        s.bind("pty-2", 24, 80);
        s.process("pty-2", b"rollacode@host codeterm % \n");
    }
    let started = std::time::Instant::now();
    let result = wait_for_echo(
        &state,
        "pty-2",
        "this-string-never-appears",
        Duration::from_millis(150),
        5,
    );
    assert!(result.is_err());
    // Sanity: we did wait roughly the timeout, not return immediately.
    assert!(started.elapsed() >= Duration::from_millis(140));
}
```

- [ ] **Step 2.4: Verify ScreenState has `bind` / `process` and `new` exposed (or expose them under cfg(test))**

Check first:
```bash
grep -n "fn bind\|fn process\|fn new" src-tauri/src/pty/screen_state.rs | head -10
```
If `pub fn bind`/`process`/`new` don't already exist, add `#[cfg(test)] pub fn new() -> Self { ... }` etc., otherwise reuse what's there. The tests at line ~150+ in screen_state.rs already construct the type — read that block and mirror its setup style.

- [ ] **Step 2.5: Run tests**

Run: `./build.sh test 2>&1 | grep -E "inject_verify|wait_for_echo|test result" | tail -15`
Expected: all four `inject_verify::tests::*` pass.

- [ ] **Step 2.6: Commit**

```bash
git add src-tauri/src/service/inject_verify.rs src-tauri/src/pty/screen_state.rs
git commit -m "feat(service): wait_for_echo + ScreenState::tail_text

Polls screen tail until expected substring appears or timeout.
40ms cadence — same order of magnitude as the existing 100ms shell-prompt
poll; cheap because tail_text only walks <= 5 rows."
```

---

## Task 3: Wait-for-submit helper (shell variant)

**Files:**
- Modify: `src-tauri/src/service/inject_verify.rs`

The "submit" check for a shell is: after Enter, a NEW prompt line (different from the one we waited for in Task 1's prompt-ready) appears. We snapshot the prompt-line content before submit, then wait for the screen to show a different last-prompt-looking line.

Note: `screen_has_shell_prompt` already exists in `pty/screen_state.rs`. Reuse it.

- [ ] **Step 3.1: Add `wait_for_submit_shell` helper**

```rust
use crate::pty::screen_state::screen_has_shell_prompt;

/// Wait until a NEW shell prompt appears on screen (i.e. the command was
/// actually submitted and either ran-and-finished or starts new output).
/// `prompt_before` is the last-non-empty-line snapshot taken just before
/// pressing Enter — we look for a prompt that differs from it OR for a
/// blank line followed by a prompt (multi-line case).
pub fn wait_for_submit_shell(
    screen_state: &ScreenStateHandle,
    pty_id: &str,
    prompt_before: &str,
    timeout: Duration,
    tail_rows: usize,
) -> Result<String, String> {
    let deadline = std::time::Instant::now() + timeout;
    let prompt_before_norm: String = prompt_before.split_whitespace().collect::<Vec<_>>().join(" ");
    loop {
        let tail = screen_state
            .lock()
            .tail_text(pty_id, tail_rows)
            .unwrap_or_default();
        if let Some(last_nonempty) = tail.lines().rev().find(|l| !l.trim().is_empty()) {
            let last_norm: String =
                last_nonempty.split_whitespace().collect::<Vec<_>>().join(" ");
            if last_norm != prompt_before_norm && screen_has_shell_prompt(&tail) {
                return Ok(tail);
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err(tail);
        }
        std::thread::sleep(Duration::from_millis(40));
    }
}

/// Snapshot helper: returns the last-non-empty line of the current tail,
/// for use as the `prompt_before` argument to `wait_for_submit_shell`.
pub fn snapshot_prompt_line(
    screen_state: &ScreenStateHandle,
    pty_id: &str,
    tail_rows: usize,
) -> String {
    let tail = screen_state
        .lock()
        .tail_text(pty_id, tail_rows)
        .unwrap_or_default();
    tail.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .to_string()
}
```

- [ ] **Step 3.2: Tests**

```rust
#[test]
fn wait_for_submit_shell_succeeds_when_new_prompt_appears() {
    use crate::pty::screen_state::ScreenState;
    use parking_lot::Mutex;
    use std::sync::Arc;
    let state = Arc::new(Mutex::new(ScreenState::new()));
    {
        let mut s = state.lock();
        s.bind("pty-3", 24, 80);
        // Initial prompt
        s.process("pty-3", b"rollacode@host codeterm % ");
    }
    let prompt_before = snapshot_prompt_line(&state, "pty-3", 5);
    assert!(prompt_before.contains("codeterm %"));
    {
        // Simulate: command was typed + Enter + ran + new prompt
        let mut s = state.lock();
        s.process("pty-3", b"echo hi\r\nhi\r\nrollacode@host other-dir % ");
    }
    let result = wait_for_submit_shell(
        &state,
        "pty-3",
        &prompt_before,
        Duration::from_millis(200),
        5,
    );
    assert!(result.is_ok(), "expected new prompt detected; got Err({:?})", result);
}

#[test]
fn wait_for_submit_shell_times_out_when_prompt_unchanged() {
    use crate::pty::screen_state::ScreenState;
    use parking_lot::Mutex;
    use std::sync::Arc;
    let state = Arc::new(Mutex::new(ScreenState::new()));
    {
        let mut s = state.lock();
        s.bind("pty-4", 24, 80);
        s.process("pty-4", b"rollacode@host codeterm % ");
    }
    let prompt_before = snapshot_prompt_line(&state, "pty-4", 5);
    let result = wait_for_submit_shell(
        &state,
        "pty-4",
        &prompt_before,
        Duration::from_millis(120),
        5,
    );
    assert!(result.is_err());
}
```

- [ ] **Step 3.3: Run tests**

Run: `./build.sh test 2>&1 | grep -E "wait_for_submit|test result" | tail -10`
Expected: both new tests pass.

- [ ] **Step 3.4: Commit**

```bash
git add src-tauri/src/service/inject_verify.rs
git commit -m "feat(service): wait_for_submit_shell — confirm Enter actually ran

Snapshots the prompt line before submit, then waits for a NEW prompt
that differs from the snapshot. Catches the 'duplicated paren' bug
and any case where Enter was eaten by a not-yet-ready shell."
```

---

## Task 4: VerifySubmitStrategy on AgentProvider

**Files:**
- Modify: `src-tauri/src/agent/provider.rs`
- Modify: `src-tauri/src/plugin/agent_claude/provider/mod.rs`
- Modify: `src-tauri/src/plugin/agent_codex/provider.rs`
- Modify: `src-tauri/src/plugin/agent_opencode/provider.rs`

For TUI agents (claude, codex, opencode) the "submit" marker is not a new shell prompt — it's the input box draining (the typed text disappears from the input area) or a "Working..." indicator appearing. We expose this as an enum so providers pick their own strategy.

- [ ] **Step 4.1: Add the enum and trait method**

Edit `src-tauri/src/agent/provider.rs`. Near `ReadinessStrategy`:

```rust
/// How to confirm that text submitted to a launched provider was actually
/// processed (Enter consumed, REPL accepted it). Used to gate `OK` on a
/// real receiver-side observation rather than an optimistic write.
#[derive(Debug, Clone)]
pub enum VerifySubmitStrategy {
    /// Wait for a new shell prompt to appear (default for plain shells and
    /// providers that pipe through the shell unchanged).
    ShellPrompt { timeout: std::time::Duration },
    /// Wait for the TUI's input box to drain — the typed text disappears
    /// from the bottom rows. Cheap heuristic: poll the last 3 rows; if
    /// they no longer contain the submitted text after `idle_after_drain`
    /// ms of stability, declare drained.
    TuiInputDrained { timeout: std::time::Duration, idle_after_drain: std::time::Duration },
    /// No verification — for providers where post-submit state is not
    /// observable from PTY (e.g. session-id-injection mode where launch
    /// is via flag, no PTY input happens at all).
    None,
}
```

In the `AgentProvider` trait, add:

```rust
fn verify_submit_strategy(&self) -> VerifySubmitStrategy {
    VerifySubmitStrategy::ShellPrompt { timeout: std::time::Duration::from_secs(5) }
}
```

The default returns `ShellPrompt` so existing providers that don't override are unchanged in behavior.

- [ ] **Step 4.2: claude provider override**

Edit `src-tauri/src/plugin/agent_claude/provider/mod.rs`. Add:

```rust
fn verify_submit_strategy(&self) -> VerifySubmitStrategy {
    VerifySubmitStrategy::TuiInputDrained {
        timeout: std::time::Duration::from_secs(15),
        idle_after_drain: std::time::Duration::from_millis(800),
    }
}
```

Update the `use` statement at the top to include `VerifySubmitStrategy`.

- [ ] **Step 4.3: codex + opencode overrides**

Same body in `agent_codex/provider.rs` and `agent_opencode/provider.rs` (matching their existing trait impl block).

- [ ] **Step 4.4: Test the trait defaults**

Add to `src-tauri/src/plugin/agent_claude/provider/mod.rs::tests`:

```rust
#[test]
fn claude_uses_tui_input_drained_for_submit_verification() {
    let p = ClaudeProvider::default();
    match p.verify_submit_strategy() {
        VerifySubmitStrategy::TuiInputDrained { timeout, idle_after_drain } => {
            assert!(timeout >= std::time::Duration::from_secs(10));
            assert!(idle_after_drain >= std::time::Duration::from_millis(500));
        }
        other => panic!("expected TuiInputDrained, got {:?}", other),
    }
}
```

- [ ] **Step 4.5: Run tests**

Run: `./build.sh test 2>&1 | grep -E "verify_submit|claude_uses_tui|test result" | tail -10`
Expected: pass.

- [ ] **Step 4.6: Commit**

```bash
git add src-tauri/src/agent/provider.rs src-tauri/src/plugin/agent_claude/ src-tauri/src/plugin/agent_codex/provider.rs src-tauri/src/plugin/agent_opencode/provider.rs
git commit -m "feat(agent): VerifySubmitStrategy trait — providers declare their submit-confirmation"
```

---

## Task 5: Wait-for-submit (TUI variant)

**Files:**
- Modify: `src-tauri/src/service/inject_verify.rs`

- [ ] **Step 5.1: Add `wait_for_submit_tui_drained`**

```rust
/// TUI variant: wait for the submitted text to LEAVE the bottom rows of
/// the screen (input box drained), then for `idle_after_drain` of no
/// further screen change to confirm the REPL has settled. Returns the
/// tail at the time of detection.
pub fn wait_for_submit_tui_drained(
    screen_state: &ScreenStateHandle,
    pty_id: &str,
    submitted_text: &str,
    timeout: Duration,
    idle_after_drain: Duration,
    tail_rows: usize,
) -> Result<String, String> {
    let deadline = std::time::Instant::now() + timeout;
    let needle: String = submitted_text.split_whitespace().collect::<Vec<_>>().join(" ");
    // Phase A: wait for the needle to disappear from the tail.
    let tail = loop {
        let t = screen_state.lock().tail_text(pty_id, tail_rows).unwrap_or_default();
        let t_norm: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
        if !t_norm.contains(&needle) || needle.is_empty() {
            break t;
        }
        if std::time::Instant::now() >= deadline {
            return Err(t);
        }
        std::thread::sleep(Duration::from_millis(40));
    };
    // Phase B: wait for `idle_after_drain` of no screen change.
    let mut last_hash = screen_state
        .lock()
        .stable_screen_hash(pty_id, 0)
        .unwrap_or_default();
    let mut stable_since = std::time::Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(40));
        let h = screen_state
            .lock()
            .stable_screen_hash(pty_id, 0)
            .unwrap_or_default();
        if h == last_hash {
            if stable_since.elapsed() >= idle_after_drain {
                return Ok(tail);
            }
        } else {
            last_hash = h;
            stable_since = std::time::Instant::now();
        }
        if std::time::Instant::now() >= deadline {
            return Err(tail);
        }
    }
}
```

Note: `stable_screen_hash` is the existing method visible in screen_state.rs around line 90 — reuse it. Verify the exact method name with:

```bash
grep -n "fn stable_screen_hash\|pub fn .*hash" src-tauri/src/pty/screen_state.rs
```

If it's named differently (e.g. `screen_hash`), update the call site accordingly.

- [ ] **Step 5.2: Tests**

```rust
#[test]
fn wait_for_submit_tui_drained_succeeds_when_needle_disappears() {
    use crate::pty::screen_state::ScreenState;
    use parking_lot::Mutex;
    use std::sync::Arc;
    let state = Arc::new(Mutex::new(ScreenState::new()));
    {
        let mut s = state.lock();
        s.bind("pty-5", 24, 80);
        // claude-like TUI: input box at bottom shows the typed prompt.
        s.process("pty-5", b"\n\n\n> hello world\n");
    }
    // Verify needle is in tail (sanity).
    let tail = state.lock().tail_text("pty-5", 5).unwrap_or_default();
    assert!(tail.contains("hello world"));
    // Now simulate Enter: input drains, "Working..." appears.
    {
        let mut s = state.lock();
        s.process("pty-5", b"\x1b[2J\x1b[H\n\nWorking...\n>\n");
    }
    let result = wait_for_submit_tui_drained(
        &state,
        "pty-5",
        "hello world",
        Duration::from_millis(500),
        Duration::from_millis(50),
        5,
    );
    assert!(result.is_ok());
}

#[test]
fn wait_for_submit_tui_drained_times_out_when_needle_persists() {
    use crate::pty::screen_state::ScreenState;
    use parking_lot::Mutex;
    use std::sync::Arc;
    let state = Arc::new(Mutex::new(ScreenState::new()));
    {
        let mut s = state.lock();
        s.bind("pty-6", 24, 80);
        s.process("pty-6", b"\n\n\n> hello world\n");
    }
    let result = wait_for_submit_tui_drained(
        &state,
        "pty-6",
        "hello world",
        Duration::from_millis(150),
        Duration::from_millis(50),
        5,
    );
    assert!(result.is_err());
}
```

- [ ] **Step 5.3: Run tests + commit**

Run: `./build.sh test 2>&1 | grep -E "wait_for_submit_tui|test result" | tail -10`

```bash
git add src-tauri/src/service/inject_verify.rs
git commit -m "feat(service): wait_for_submit_tui_drained for TUI agents (claude, codex, opencode)"
```

---

## Task 6: send_text_verified core

**Files:**
- Modify: `src-tauri/src/service/pty_io.rs`

- [ ] **Step 6.1: Add the public method**

In `impl ServiceContext { ... }` of pty_io.rs, after `send_text_to_pty`:

```rust
/// Verified flavor of [`send_text_to_pty`]. After the bracket-paste write,
/// reads the screen back to confirm the text is visible (echoed) and
/// — when `enter` is true — that submission landed via the strategy
/// chosen by the caller.
///
/// Returns `Err(InjectionError)` on echo or submit timeout. The fast happy
/// path adds at most ~50ms over the unverified send when the receiver is
/// already idle.
pub fn send_text_verified(
    &self,
    pty_id: &str,
    text: &str,
    enter: bool,
    submit_strategy: crate::agent::provider::VerifySubmitStrategy,
    opts: crate::service::inject_verify::VerifyOpts,
) -> Result<crate::service::inject_verify::InjectionResult, crate::service::inject_verify::InjectionError> {
    use crate::service::inject_verify::{
        wait_for_echo, wait_for_submit_shell, wait_for_submit_tui_drained, snapshot_prompt_line,
        InjectionError, InjectionResult,
    };
    use crate::agent::provider::VerifySubmitStrategy;
    let started = std::time::Instant::now();

    // 1. Optionally wait for shell prompt before we type.
    if opts.require_prompt_first {
        let ready = crate::service::pty_io::poll_shell_prompt(
            &self.screen_state,
            pty_id,
            opts.prompt_timeout,
        );
        if !ready {
            return Err(InjectionError::PromptNeverReady { waited: started.elapsed() });
        }
    }

    // 2. Snapshot the prompt line (used by ShellPrompt submit strategy below).
    let prompt_before = snapshot_prompt_line(&self.screen_state, pty_id, 5);

    // 3. Bracket-paste write (no Enter yet).
    let paste = format!("\x1b[200~{}\x1b[201~", text);
    self.pty.lock().write(pty_id, paste.as_bytes())
        .map_err(|_| InjectionError::PtyClosed { pty_id: pty_id.to_string() })?;
    let bytes_written = paste.len();

    // 4. Verify echo if requested.
    let mut echo_observed = false;
    if opts.verify_echo {
        match wait_for_echo(&self.screen_state, pty_id, text, opts.echo_timeout, 8) {
            Ok(_tail) => echo_observed = true,
            Err(tail) => return Err(InjectionError::EchoMismatch {
                expected_substr: text.chars().take(80).collect(),
                screen_tail: tail.chars().take(400).collect(),
            }),
        }
    }

    // 5. Submit + verify if Enter requested.
    let mut submit_observed = false;
    if enter {
        // 5a. Send \r after a short settle (mirrors existing 500ms behavior;
        //     keeps TUI bracket-paste handling unchanged).
        std::thread::sleep(std::time::Duration::from_millis(120));
        self.pty.lock().write(pty_id, b"\r")
            .map_err(|_| InjectionError::PtyClosed { pty_id: pty_id.to_string() })?;

        if opts.verify_submit {
            let res = match submit_strategy {
                VerifySubmitStrategy::ShellPrompt { timeout } => {
                    wait_for_submit_shell(&self.screen_state, pty_id, &prompt_before, timeout, 5)
                }
                VerifySubmitStrategy::TuiInputDrained { timeout, idle_after_drain } => {
                    wait_for_submit_tui_drained(&self.screen_state, pty_id, text, timeout, idle_after_drain, 8)
                }
                VerifySubmitStrategy::None => Ok(String::new()),
            };
            match res {
                Ok(_) => submit_observed = true,
                Err(tail) => return Err(InjectionError::SubmitNeverConfirmed {
                    waited: started.elapsed(),
                    screen_tail: tail.chars().take(400).collect(),
                }),
            }
        }
    }

    Ok(InjectionResult {
        bytes_written,
        echo_observed,
        submit_observed,
        elapsed: started.elapsed(),
    })
}
```

- [ ] **Step 6.2: Test the full path with a synthetic PTY/screen pair**

Skip — no clean way to drive the real PtyManager from a unit test. The integration is exercised end-to-end by the existing `spawn_tests.rs` once we wire orchestrator over to the verified path in Task 7.

- [ ] **Step 6.3: Compile-only check + commit**

Run: `./build.sh check 2>&1 | tail -10`
Expected: no warnings, no errors related to inject_verify.

```bash
git add src-tauri/src/service/pty_io.rs
git commit -m "feat(service): send_text_verified — bracket-paste + echo/submit gating"
```

---

## Task 7: Wire orchestrator launch_agent through send_text_verified

**Files:**
- Modify: `src-tauri/src/service/orchestrator.rs` (around line 470-540, the `launch_agent` background block)

The current flow does `poll_shell_prompt → send_text_to_pty(launch_cmd) → 500ms sleep → send_text_to_pty(task)`. Replace with a single `send_text_verified` call that handles prompt+echo+submit, then a second `send_text_verified` for the task with `require_prompt_first=false` (the agent is already running, no shell prompt to wait for) and `submit_strategy = provider.verify_submit_strategy()`.

The `tokio::spawn`/`std::thread::spawn` block also needs to push its outcome to a oneshot so the REST handler can await it (Task 9).

- [ ] **Step 7.1: Refactor launch_agent body**

Read `src-tauri/src/service/orchestrator.rs:470-540` carefully. Replace the body that does `match readiness { … } / send_text_to_pty(launch_cmd) / sleep / send_text_to_pty(task)` with:

```rust
let provider_submit_strategy = registry
    .with_provider(&agent_cmd, |p| p.verify_submit_strategy())
    .unwrap_or(crate::agent::provider::VerifySubmitStrategy::ShellPrompt {
        timeout: std::time::Duration::from_secs(5),
    });
let opts = crate::service::inject_verify::VerifyOpts {
    require_prompt_first: matches!(readiness,
        crate::agent::provider::ReadinessStrategy::PollShellPrompt { .. }),
    verify_echo: true,
    verify_submit: true,
    prompt_timeout: match readiness {
        crate::agent::provider::ReadinessStrategy::PollShellPrompt { timeout } => timeout,
        _ => std::time::Duration::from_secs(2),
    },
    echo_timeout: std::time::Duration::from_secs(3),
    submit_timeout: std::time::Duration::from_secs(15),
};

let launch_result = svc.send_text_verified(
    &pty_id,
    &launch_cmd,
    true,
    // Launch line goes through the SHELL — it's a `claude --flags` invocation,
    // not TUI input. Use ShellPrompt verification regardless of provider.
    crate::agent::provider::VerifySubmitStrategy::ShellPrompt {
        timeout: std::time::Duration::from_secs(15),
    },
    opts.clone(),
);
if let Err(e) = launch_result {
    log::error!("launch_agent: launch_cmd failed for pane {} (pty {}): {}", pane_id, pty_id, e);
    let _ = ready_tx.send(Err(format!("launch_cmd failed: {}", e)));
    return;
}

if matches!(delivery, crate::agent::provider::TaskDelivery::PtyInput) {
    if let Some(ref task_text) = task {
        let task_opts = crate::service::inject_verify::VerifyOpts {
            require_prompt_first: false,
            verify_echo: true,
            verify_submit: true,
            prompt_timeout: std::time::Duration::from_millis(0),
            echo_timeout: std::time::Duration::from_secs(3),
            submit_timeout: std::time::Duration::from_secs(15),
        };
        match svc.send_text_verified(&pty_id, task_text, true, provider_submit_strategy, task_opts) {
            Ok(_) => {}
            Err(e) => {
                log::error!("launch_agent: task delivery failed for pane {}: {}", pane_id, e);
                let _ = ready_tx.send(Err(format!("task delivery failed: {}", e)));
                return;
            }
        }
    }
}

log::info!("launch_agent: agent ready in pane {}", pane_id);
let _ = ready_tx.send(Ok(()));
```

`ready_tx` is a `tokio::sync::oneshot::Sender<Result<(), String>>` plumbed in from the caller (Task 9 will add the matching receiver in the REST handler). For now, define it inside `launch_agent` if no external caller wants it yet, and expose an overload that takes one optionally.

- [ ] **Step 7.2: Compile + check**

Run: `./build.sh check 2>&1 | tail -15`
Expected: clean.

- [ ] **Step 7.3: Run all tests**

Run: `./build.sh test 2>&1 | tail -5`
Expected: all green; existing spawn_tests still pass.

- [ ] **Step 7.4: Commit**

```bash
git add src-tauri/src/service/orchestrator.rs
git commit -m "refactor(orchestrator): launch_agent uses send_text_verified

Replaces the optimistic 'fire-and-forget' launch_cmd + 500ms sleep + task
sequence with two verified send calls, each gated on echo and submit
confirmation. ready_tx oneshot lets REST handlers await actual
readiness in a follow-up commit."
```

---

## Task 8: Wire spawn.rs hidden-spawn path

**Files:**
- Modify: `src-tauri/src/service/spawn.rs:649` (the `if let Err(e) = svc_outer.send_text(&pane_id_outer, &launch_cmd, true)` block)

- [ ] **Step 8.1: Replace the optimistic send_text with send_text_verified**

```rust
let pty_id_for_send = svc_outer.resolve_pty_id(&pane_id_outer)
    .map_err(|e| log::error!("[spawn.hidden] no pty for pane {}: {}", pane_id_outer, e));
if let Ok(pty_id) = pty_id_for_send {
    let opts = crate::service::inject_verify::VerifyOpts::default();
    if let Err(e) = svc_outer.send_text_verified(
        &pty_id,
        &launch_cmd,
        true,
        crate::agent::provider::VerifySubmitStrategy::ShellPrompt {
            timeout: std::time::Duration::from_secs(15),
        },
        opts,
    ) {
        log::error!("[spawn.hidden] verified send launch cmd: {}", e);
    }
}
```

- [ ] **Step 8.2: Tests + commit**

Run: `./build.sh test 2>&1 | tail -5`

```bash
git add src-tauri/src/service/spawn.rs
git commit -m "refactor(spawn): hidden-spawn path uses send_text_verified"
```

---

## Task 9: Block agent-spawn REST until ready (or timeout)

**Files:**
- Modify: `src-tauri/src/api/agent_routes.rs` (verify path: `grep -rn "spawn_agent\b" src-tauri/src/api/`)
- Modify: `src-tauri/src/service/orchestrator.rs` — `spawn_agent` accepts an optional `ready_tx` and forwards it to `launch_agent`.

The REST handler creates a oneshot `(tx, rx)`, passes `tx` into `spawn_agent`, then `tokio::time::timeout(30s, rx).await`. Returns the result or a timeout error in JSON.

- [ ] **Step 9.1: Plumb the oneshot through `spawn_agent` → `launch_agent`**

Edit `orchestrator.rs::spawn_agent` to accept `ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>` and pass it into `launch_agent`. `launch_agent` already has it from Task 7.

- [ ] **Step 9.2: Update REST handler**

Find the `spawn_agent` REST handler. After the existing call to `state.svc.spawn_agent(...)`:

```rust
let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
let handle = state.svc.spawn_agent(spec, Some(ready_tx))
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

let wait = tokio::time::timeout(std::time::Duration::from_secs(30), ready_rx).await;
let outcome = match wait {
    Ok(Ok(Ok(()))) => serde_json::json!({"ready": true}),
    Ok(Ok(Err(e))) => serde_json::json!({"ready": false, "error": e}),
    Ok(Err(_recv_err)) => serde_json::json!({"ready": false, "error": "internal: launch thread dropped"}),
    Err(_timeout) => serde_json::json!({"ready": false, "error": "timed out waiting for agent to become ready"}),
};

Ok(Json(serde_json::json!({
    "pane_id": handle.pane_id,
    "tab_id": handle.tab_id,
    "workspace_id": handle.workspace_id,
    "agent": outcome,
})))
```

The CLI gets `agent.ready=true` only on real readiness; otherwise `agent.ready=false` with a reason. Existing call sites that ignore the new field keep working.

- [ ] **Step 9.3: Tests**

Add a unit/integration test in `src-tauri/src/api/agent_routes_tests.rs` (or wherever the existing tests for the route live) that:
- Stubs out `spawn_agent` with a fake that fires `ready_tx` with `Err`.
- Asserts the REST handler responds with `agent.ready=false`.

If the existing test scaffold can't easily inject a stub, just rely on the end-to-end behavior change — captured in Task 12's e2e test.

- [ ] **Step 9.4: Commit**

```bash
git add src-tauri/src/api/agent_routes.rs src-tauri/src/service/orchestrator.rs
git commit -m "feat(api): /api/agents/spawn awaits readiness before returning

Adds a oneshot channel between launch_agent and the REST handler. The
handler waits up to 30s for the agent to confirm ready (echo + submit
verified) and returns agent.ready in the response body. Old clients
that ignore this field are unaffected."
```

---

## Task 10: Verified send_text REST endpoint

**Files:**
- Modify: `src-tauri/src/api/pane_routes.rs` (the `send_text` handler around line 85)

- [ ] **Step 10.1: Extend SendTextBody**

```rust
#[derive(Deserialize)]
pub(crate) struct SendTextBody {
    text: Option<String>,
    #[serde(default = "default_true")]
    enter: bool,
    batch: Option<Vec<BatchCommand>>,
    #[serde(default)]
    raw: bool,
    /// When true (default), block until echo/submit are confirmed and
    /// return verified=true|false in the response. When false, behave like
    /// the legacy fire-and-forget path.
    #[serde(default = "default_true")]
    verify: bool,
}
```

- [ ] **Step 10.2: Dispatch to send_text_verified when verify=true**

In the handler body, after auth + pane resolution:

```rust
if body.verify && body.batch.is_none() && !body.raw {
    let pty_id = state.svc.resolve_pty_id(&pane_id)
        .map_err(|_| (StatusCode::NOT_FOUND, "pane has no PTY"))?;
    let text = body.text.unwrap_or_default();
    let opts = crate::service::inject_verify::VerifyOpts::default();
    let strategy = crate::agent::provider::VerifySubmitStrategy::ShellPrompt {
        timeout: std::time::Duration::from_secs(10),
    };
    return match state.svc.send_text_verified(&pty_id, &text, body.enter, strategy, opts) {
        Ok(r) => Ok(Json(serde_json::json!({
            "ok": true,
            "verified": true,
            "echo_observed": r.echo_observed,
            "submit_observed": r.submit_observed,
            "elapsed_ms": r.elapsed.as_millis() as u64,
        }))),
        Err(e) => Ok(Json(serde_json::json!({
            "ok": false,
            "verified": false,
            "error": format!("{}", e),
        }))),
    };
}
// legacy path follows...
```

- [ ] **Step 10.3: Tests + commit**

Add a test that asserts the body shape with `verify=false` keeps the legacy response unchanged. Run `./build.sh test`, then commit.

```bash
git add src-tauri/src/api/pane_routes.rs
git commit -m "feat(api): /api/panes/{id}/send verify=true blocks on echo+submit

Default verify=true gives 'OK = really happened' semantics. Pass
verify=false to opt back into the legacy fire-and-forget behavior."
```

---

## Task 11: CLI surfaces verified result + relaxed shell-mangle guard

**Files:**
- Modify: `cli/src/main.rs` — `guard_against_shell_mangle`
- Modify: `cli/src/cmd/send_text.rs` (or wherever the human/JSON formatter is — verify with grep)
- Modify: `cli/src/cmd/agent.rs` — same for spawn output

- [ ] **Step 11.1: Relax `guard_against_shell_mangle` for fenced code blocks**

Replace the body:

```rust
fn guard_against_shell_mangle(text: &str) -> Result<(), String> {
    const SUSPICIOUS: &[&str] = &[
        ": command not found",
        "/usr/bin/bash: line ",
        "bash: line ",
        "syntax error near unexpected token",
    ];
    let scrubbed = strip_fenced_code_blocks(text);
    for needle in SUSPICIOUS {
        if scrubbed.contains(needle) {
            return Err(format!(
                "send-text payload looks shell-mangled (matched marker: {:?})",
                needle
            ));
        }
    }
    Ok(())
}

/// Removes the contents of all triple-backtick fenced code blocks from
/// `text`, leaving the surrounding prose. Used to keep the shell-mangle
/// guard from firing on legitimate code-block content (e.g. error
/// messages quoted in documentation or repro steps).
fn strip_fenced_code_blocks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_block = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_block = !in_block;
            continue;
        }
        if !in_block {
            out.push_str(line);
        }
    }
    out
}
```

- [ ] **Step 11.2: Tests for the guard**

Append to `cli/src/main.rs::tests`:

```rust
#[test]
fn guard_ignores_marker_inside_fenced_code_block() {
    let payload = "Here is the repro:\n```\npunset: command not found\n```\nEnd.";
    assert!(guard_against_shell_mangle(payload).is_ok());
}

#[test]
fn guard_still_fires_on_marker_outside_code_block() {
    let payload = "this is just /usr/bin/bash: line 5: foo not in a code block";
    assert!(guard_against_shell_mangle(payload).is_err());
}

#[test]
fn guard_handles_unterminated_fence_safely() {
    // If the fence is never closed, treat the rest as code-block (paranoid
    // — better to skip a guard than spuriously reject).
    let payload = "```\n: command not found\nstill open\n";
    assert!(guard_against_shell_mangle(payload).is_ok());
}

#[test]
fn guard_with_multiple_fences_strips_only_inside() {
    let payload = "outside ok\n```\n: command not found\n```\nbetween\n```\nbash: line 1: x\n```\ntrailing";
    assert!(guard_against_shell_mangle(payload).is_ok());
}
```

- [ ] **Step 11.3: Surface verified fields in CLI output**

For `send-text`: read the JSON response. If `verified=true`, print `OK (echo+submit confirmed in <N>ms)`. If `verified=false`, print the `error` and exit nonzero. Find the existing print-on-success site in `cli/src/cmd/send_text.rs` (or `cli/src/main.rs` if not yet split) and update.

For `agent spawn`: when REST returns `agent.ready=true`, keep the existing `Spawned: <Title> (pane <id>)` line. When `agent.ready=false`, print `Spawn failed: <reason>` and exit 1.

- [ ] **Step 11.4: --no-wait flag**

Add to the agent-spawn and send-text CLI arg structs (`cli/src/args/agent.rs`, `cli/src/args/send_text.rs` or wherever):

```rust
/// Skip readiness verification and return immediately after the request is
/// accepted. Useful for fire-and-forget scripts. Default: wait.
#[arg(long, default_value_t = false)]
pub no_wait: bool,
```

When `--no-wait` is set, the CLI sends `verify=false` in the send-text body and skips reading the `agent.ready` field on spawn responses (just prints the pane_id).

- [ ] **Step 11.5: Run tests + cargo build the CLI**

```bash
./build.sh test 2>&1 | tail -5
./build.sh cli 2>&1 | tail -5
```

- [ ] **Step 11.6: Commit**

```bash
git add cli/src/
git commit -m "feat(cli): surface verified spawn/send-text results + --no-wait + relaxed mangle guard

- guard_against_shell_mangle now ignores markers inside fenced code blocks,
  so docs/repros that quote 'command not found' lines pass through.
- send-text and agent spawn print 'OK (verified in Nms)' on success and
  exit nonzero when the daemon returns verified=false.
- --no-wait flag opts back into the legacy fire-and-forget behavior."
```

---

## Task 12: End-to-end verify on a live dev daemon

**Files:** none — this is the smoke test.

- [ ] **Step 12.1: Restart dev with the new build**

Run: `./build.sh dev` and wait for `curl -s http://127.0.0.1:7686/api/health | jq .build` to match `git rev-parse HEAD`.

- [ ] **Step 12.2: Spawn a claude pane via CLI and confirm verified=true**

Pick a workspace ID from `~/.codeterm-dev/bin/codeterm --json workspace list`.
Run:

```bash
~/.codeterm-dev/bin/codeterm agent spawn claude \
    --title 'spawn-readiness-smoke' \
    --workspace <WS_ID> \
    --task 'just say hi' \
    --json
```

Inspect the JSON response. Expected: `agent.ready=true`. The CLI exits 0.

- [ ] **Step 12.3: get-text on the new pane and confirm no corruption**

```bash
~/.codeterm-dev/bin/codeterm pane get-text --pane <PANE_ID> | tail -20
```

Expected: full launch line `unset CLAUDECODE && claude --dangerously-skip-permissions "$(cat /tmp/.ct-task-XXXXXX)"` appears intact, followed by the claude UI banner. No `codeterpunset`, no duplicate parens.

- [ ] **Step 12.4: send-text into a shell pane and confirm verified=true**

Spawn a plain shell pane, then:

```bash
~/.codeterm-dev/bin/codeterm send-text --pane <SHELL_PANE_ID> --text 'echo verified-test-$$' --json
```

Expected JSON: `{"ok":true,"verified":true,"echo_observed":true,"submit_observed":true,"elapsed_ms": <small number>}`.

- [ ] **Step 12.5: --no-wait fast-path**

```bash
~/.codeterm-dev/bin/codeterm send-text --pane <SHELL_PANE_ID> --text 'sleep 2 && echo done' --no-wait --json
```

Expected: returns immediately with `verified=false` (or a `verified` key absent), CLI exits 0.

- [ ] **Step 12.6: Negative case — induce an echo timeout**

```bash
# Find a pane with no shell (e.g. just-spawned, no PTY ready yet) and send-text into it.
# Or, easier: kill the PTY then send-text.
~/.codeterm-dev/bin/codeterm send-text --pane <BAD_PANE_ID> --text 'never lands' --json
```

Expected: `{"ok":false,"verified":false,"error":"shell prompt never appeared..."}` (or PtyClosed depending on timing). CLI exits nonzero.

- [ ] **Step 12.7: Telegram milestone**

```bash
~/.codeterm-dev/bin/codeterm bridge send --to 186809246 --service telegram \
    "spawn-readiness fix: end-to-end smoke green — agent spawn / send-text both surface verified=true on real success and verified=false with a clear reason on timeout. No auto-commit; ready for review."
```

- [ ] **Step 12.8: Final agent report**

```bash
codeterm agent report 'spawn-readiness fix: PASS — N tests added, all spawn/inject paths gated on real readiness. Diffs ready for Andrey review on chore/cli-dev-bin-and-mem-slim.'
```

(`N` = sum of new test cases added in tasks 1–11. Recount before reporting.)

---

## Self-Review Checklist (run by author before handoff)

- [ ] Spec coverage: every requirement (1)–(5) from Andrey's request maps to a task.
  - (1) child pane created — already done by `spawn_pane`, untouched.
  - (2) child shell ready before injection — Task 6 step 1, gated by `poll_shell_prompt`.
  - (3) text fully typed in — Task 6 step 4 (echo verification).
  - (4) submission happened — Task 6 step 5 + Task 3 + Task 5.
  - (5) claude received the prompt — Task 4/5 (TuiInputDrained).
- [ ] No `TODO`, `TBD`, "fill in" placeholders remain.
- [ ] Type names consistent across tasks: `VerifyOpts`, `InjectionResult`, `InjectionError`, `VerifySubmitStrategy`, `send_text_verified` — all match exactly between definition and usage.
- [ ] Method names match real source: `tail_text`, `arm_prompt_signal`, `screen_has_shell_prompt`, `stable_screen_hash` — verified before claiming done; if any differ, update the call sites.
- [ ] Cross-platform: no macOS-only paths; bracket-paste works the same on Linux + Windows ConPTY (already shipped behavior).
- [ ] Lock ordering preserved: `screen_state` is acquired AFTER `mux` in existing code; the new helpers acquire only `screen_state` and `pty`, never both nested. No `screen_state` held across awaits — all polling does `lock(); read; drop; sleep`.
