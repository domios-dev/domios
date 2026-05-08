use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;

use crate::ScreenStateHandle;
use crate::pty::screen_state::screen_has_shell_prompt;

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
            Self::PromptNeverReady { waited } => {
                write!(f, "prompt never became ready after {:?}", waited)
            }
            Self::EchoMismatch { expected_substr, screen_tail } => {
                write!(
                    f,
                    "expected echo not found on screen; expected substring={:?}; screen_tail={:?}",
                    expected_substr, screen_tail
                )
            }
            Self::SubmitNeverConfirmed { waited, screen_tail } => {
                write!(
                    f,
                    "submit not confirmed after {:?}; screen_tail={:?}",
                    waited, screen_tail
                )
            }
            Self::PtyClosed { pty_id } => write!(f, "pty {} is closed", pty_id),
        }
    }
}

impl std::error::Error for InjectionError {}

const POLL_INTERVAL: Duration = Duration::from_millis(40);

#[derive(Debug, Clone)]
pub enum HelperErr {
    Timeout(String),    // screen tail captured at timeout
    PtyUnknown(String), // pty_id with no bound parser
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn last_nonempty_line(s: &str) -> &str {
    s.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("")
}

/// Matches the bracketed-paste placeholder rendered by Claude Code, Codex,
/// and other modern TUIs. Two render shapes exist depending on payload:
///   `[Pasted text #1]`           — single-line paste
///   `[Pasted text #1 +12 lines]` — multi-line paste (`+N lines` suffix)
/// When the TUI shows either shape, the literal pasted text never appears on
/// screen, so substring search against `expected` will never match.
fn paste_placeholder_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[Pasted text #\d+( \+\d+ lines?)?\]").unwrap())
}

fn screen_has_paste_placeholder(screen: &str) -> bool {
    paste_placeholder_regex().is_match(screen)
}

fn last_n_chars(s: &str, n: usize) -> String {
    let total = s.chars().count();
    let skip = total.saturating_sub(n);
    s.chars().skip(skip).collect()
}

const ECHO_SUFFIX_MATCH_CHARS: usize = 80;
const ECHO_LONG_INPUT_BYTES: usize = 200;
/// Only fall back to suffix-match when the paste is large enough that the
/// prefix could realistically have scrolled out of the tail-rows window.
/// Tail capacity is ~8 rows × ~100 cols ≈ 800 bytes; require a clear margin
/// above that so corruption in normal-size inputs is still detected.
const ECHO_SUFFIX_MATCH_MIN_INPUT_BYTES: usize = 1024;

pub fn screen_contains_echo(screen_tail: &str, expected: &str) -> bool {
    if expected.is_empty() {
        return true;
    }
    let n_expected = normalize_ws(expected);
    let n_tail = normalize_ws(screen_tail);
    if n_tail.contains(&n_expected) {
        return true;
    }
    let flat = screen_tail.replace('\n', "");
    let n_flat = normalize_ws(&flat);
    if n_flat.contains(&n_expected) {
        return true;
    }
    // Long shell paste: prefix scrolls out of the tail-rows window before we
    // poll, so the head of `expected` is never visible. The tail of the paste
    // still is — match the last N chars instead. Gated on input size so a
    // ~90-char command with a corrupted prefix is still caught by the strict
    // substring check above.
    if expected.len() >= ECHO_SUFFIX_MATCH_MIN_INPUT_BYTES {
        let suffix = last_n_chars(&n_expected, ECHO_SUFFIX_MATCH_CHARS);
        if n_tail.contains(&suffix) || n_flat.contains(&suffix) {
            return true;
        }
    }
    // Bracketed-paste TUI placeholder: Claude/Codex collapse multi-line pastes
    // into "[Pasted text #N +M lines]" — literal text never appears, but the
    // paste was accepted. Treat the placeholder as echo confirmation, but only
    // for inputs that would actually trigger the placeholder rendering.
    let placeholder_eligible = expected.contains('\n') || expected.len() > ECHO_LONG_INPUT_BYTES;
    if placeholder_eligible && screen_has_paste_placeholder(screen_tail) {
        return true;
    }
    false
}

pub fn wait_for_echo(
    screen_state: &ScreenStateHandle,
    pty_id: &str,
    expected: &str,
    timeout: Duration,
    tail_rows: usize,
) -> Result<String, HelperErr> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let tail = match screen_state.lock().tail_text(pty_id, tail_rows) {
            Some(t) => t,
            None => return Err(HelperErr::PtyUnknown(pty_id.to_string())),
        };
        if screen_contains_echo(&tail, expected) {
            return Ok(tail);
        }
        if std::time::Instant::now() >= deadline {
            return Err(HelperErr::Timeout(tail));
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

pub fn snapshot_prompt_line(
    screen_state: &ScreenStateHandle,
    pty_id: &str,
    tail_rows: usize,
) -> String {
    let tail = screen_state
        .lock()
        .tail_text(pty_id, tail_rows)
        .unwrap_or_default();
    last_nonempty_line(&tail).to_string()
}

pub fn snapshot_screen_hash(screen_state: &ScreenStateHandle, pty_id: &str) -> Option<[u8; 32]> {
    screen_state.lock().get_content_hash(pty_id, 0)
}

pub fn wait_for_submit_shell(
    screen_state: &ScreenStateHandle,
    pty_id: &str,
    hash_before: [u8; 32],
    timeout: Duration,
    tail_rows: usize,
) -> Result<String, HelperErr> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if std::time::Instant::now() >= deadline {
            let tail = screen_state.lock().tail_text(pty_id, tail_rows).unwrap_or_default();
            return Err(HelperErr::Timeout(tail));
        }
        let (tail_opt, hash_now) = {
            let s = screen_state.lock();
            (s.tail_text(pty_id, tail_rows), s.get_content_hash(pty_id, 0))
        };
        let tail = match tail_opt {
            Some(t) => t,
            None => return Err(HelperErr::PtyUnknown(pty_id.to_string())),
        };
        if let Some(h) = hash_now {
            if h != hash_before && screen_has_shell_prompt(&tail) {
                return Ok(tail);
            }
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

pub fn wait_for_submit_tui_drained(
    screen_state: &ScreenStateHandle,
    pty_id: &str,
    submitted_text: &str,
    timeout: Duration,
    idle_after_drain: Duration,
    tail_rows: usize,
) -> Result<String, HelperErr> {
    let deadline = std::time::Instant::now() + timeout;
    let needle = normalize_ws(submitted_text);

    let drained_tail = loop {
        if std::time::Instant::now() >= deadline {
            // Capture last-known tail (may be empty if pty vanished).
            let last = screen_state
                .lock()
                .tail_text(pty_id, tail_rows)
                .unwrap_or_default();
            return Err(HelperErr::Timeout(last));
        }
        let t = match screen_state.lock().tail_text(pty_id, tail_rows) {
            Some(t) => t,
            None => return Err(HelperErr::PtyUnknown(pty_id.to_string())),
        };
        let t_norm = normalize_ws(&t);
        if needle.is_empty() || !t_norm.contains(&needle) {
            break t;
        }
        std::thread::sleep(POLL_INTERVAL);
    };

    let mut last_hash = match screen_state.lock().get_content_hash(pty_id, 0) {
        Some(h) => h,
        None => return Err(HelperErr::PtyUnknown(pty_id.to_string())),
    };
    let mut stable_since = std::time::Instant::now();
    loop {
        if std::time::Instant::now() >= deadline {
            return Err(HelperErr::Timeout(drained_tail));
        }
        std::thread::sleep(POLL_INTERVAL);
        let h = match screen_state.lock().get_content_hash(pty_id, 0) {
            Some(h) => h,
            None => return Err(HelperErr::PtyUnknown(pty_id.to_string())),
        };
        if h == last_hash {
            if stable_since.elapsed() >= idle_after_drain {
                return Ok(drained_tail);
            }
        } else {
            last_hash = h;
            stable_since = std::time::Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::screen_state::ScreenState;
    use parking_lot::Mutex;
    use std::sync::Arc;

    fn handle() -> ScreenStateHandle {
        Arc::new(Mutex::new(ScreenState::new()))
    }

    fn feed(h: &ScreenStateHandle, pty: &str, bytes: &[u8]) {
        h.lock().process(pty, bytes);
    }

    #[test]
    fn screen_contains_echo_handles_simple_match() {
        assert!(screen_contains_echo("user@host:~$ ls -la", "ls -la"));
    }

    #[test]
    fn screen_contains_echo_normalizes_whitespace() {
        assert!(screen_contains_echo(
            "user@host:~$  ls    -la\n",
            "ls -la",
        ));
        assert!(screen_contains_echo(
            "line1\n   user@host:~$\tls\t-la\nline3",
            "ls -la",
        ));
    }

    #[test]
    fn screen_contains_echo_returns_true_for_empty_expected() {
        assert!(screen_contains_echo("anything here", ""));
        assert!(screen_contains_echo("", ""));
    }

    #[test]
    fn observed_corruption_is_detected_as_missing_echo() {
        // Prompt overwrote first chars: "nset" instead of "unset" — substring miss.
        let screen = "rollacode@host codeterm % nset CLAUDECODE && claude --dangerously-skip-permissions \"$(cat /tmp/.ct-task-01219c41)\")\"";
        let expected = r#"unset CLAUDECODE && claude --dangerously-skip-permissions "$(cat /tmp/.ct-task-01219c41)""#;
        assert!(!screen_contains_echo(screen, expected));
    }

    #[test]
    fn duplicated_trailing_paren_passes_echo_check() {
        // Substring still matches with duplicated `)"` tail; submit verify catches it.
        let screen = "rollacode@host codeterm % unset CLAUDECODE && claude --dangerously-skip-permissions \"$(cat /tmp/.ct-task-01219c41)\")\"";
        let expected = r#"unset CLAUDECODE && claude --dangerously-skip-permissions "$(cat /tmp/.ct-task-01219c41)""#;
        assert!(screen_contains_echo(screen, expected));
    }

    #[test]
    fn long_paste_with_scrolled_out_prefix_matches_via_suffix() {
        // 5000-char paste: only the trailing portion fits into the 8-row tail
        // window. Echo verify must still confirm via suffix match.
        let body: String = "A".repeat(4900);
        let expected = format!("[Mesh: BLACKSTAR pane=abc12345] {} TAIL_TOKEN_END", body);
        // Tail captured by screen_state contains only the last ~640 chars of the paste.
        let visible_tail: String = expected.chars().rev().take(600).collect::<String>().chars().rev().collect();
        assert!(
            screen_contains_echo(&visible_tail, &expected),
            "long paste should match via suffix even when prefix scrolled out"
        );
    }

    #[test]
    fn bracketed_paste_placeholder_counts_as_echo_for_multiline_input() {
        // Claude TUI replaces multi-line pastes with the placeholder; literal
        // text never appears on screen.
        let screen = "> [Pasted text #1 +12 lines]\n  Press Enter to send";
        let expected = "line one\nline two\nline three\nline four";
        assert!(
            screen_contains_echo(screen, expected),
            "multi-line paste replaced by placeholder must count as echo"
        );
    }

    #[test]
    fn bracketed_paste_placeholder_counts_as_echo_for_long_input() {
        let screen = "> [Pasted text #7 +1 line]\n  Press Enter to send";
        let expected = format!("[Mesh: PEER pane=abc12345] {}", "x".repeat(500));
        assert!(
            screen_contains_echo(screen, &expected),
            "long single-line paste replaced by placeholder must count as echo"
        );
    }

    #[test]
    fn bracketed_paste_placeholder_without_lines_suffix_counts_as_echo() {
        // Real Claude Code render for a long single-line paste with no
        // newlines is `[Pasted text #N]` WITHOUT the `+N lines` suffix.
        // v1.1.18 regex required the suffix and missed this case entirely,
        // which is why the v1.1.18 smoke test failed for ~4.7KB single-line
        // mesh messages.
        let screen = "> [Pasted text #1]";
        let expected = format!("[Mesh: PEER pane=abc12345] {}", "x".repeat(4500));
        assert!(
            screen_contains_echo(screen, &expected),
            "single-line paste without `+N lines` suffix must still count as echo"
        );
    }

    #[test]
    fn placeholder_does_not_falsely_match_short_single_line_input() {
        // Defensive: a literal "[Pasted text #N +M lines]" on screen must not
        // confirm an unrelated short input.
        let screen = "> [Pasted text #3 +2 lines]";
        let expected = "ls -la";
        assert!(
            !screen_contains_echo(screen, expected),
            "placeholder must not satisfy a short single-line input"
        );
    }

    #[test]
    fn placeholder_regex_matches_singular_and_plural_lines() {
        assert!(screen_has_paste_placeholder("> [Pasted text #1 +1 line]"));
        assert!(screen_has_paste_placeholder("> [Pasted text #99 +250 lines]"));
        // v1.1.19 widened the regex to also accept the bare `[Pasted text #N]`
        // shape (without the trailing `+M lines` suffix).
        assert!(screen_has_paste_placeholder("> [Pasted text #1]"));
        // Bracket pair is required; the unbracketed prose form must not match.
        assert!(!screen_has_paste_placeholder("Pasted text #1 +1 line"));
    }

    // vt100 default is 50 rows; tail_rows >= 50 scans the whole screen.
    const TEST_TAIL_ROWS: usize = 60;

    #[test]
    fn wait_for_echo_returns_immediately_when_text_already_present() {
        let h = handle();
        feed(&h, "p1", b"user@host:~$ ls -la\n");
        let start = std::time::Instant::now();
        let res = wait_for_echo(&h, "p1", "ls -la", Duration::from_secs(1), TEST_TAIL_ROWS);
        assert!(res.is_ok());
        assert!(start.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn wait_for_echo_times_out_when_text_never_arrives() {
        let h = handle();
        feed(&h, "p1", b"user@host:~$ \n");
        let start = std::time::Instant::now();
        let res = wait_for_echo(
            &h,
            "p1",
            "missing-token",
            Duration::from_millis(150),
            TEST_TAIL_ROWS,
        );
        assert!(matches!(res, Err(HelperErr::Timeout(_))), "got {:?}", res);
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(140),
            "elapsed {:?}",
            elapsed
        );
        assert!(
            elapsed < Duration::from_millis(250),
            "elapsed {:?} too long — deadline check might be broken",
            elapsed
        );
    }

    #[test]
    fn wait_for_echo_returns_pty_unknown_when_pty_not_bound() {
        let h = handle();
        let start = std::time::Instant::now();
        let res = wait_for_echo(
            &h,
            "nonexistent-pty",
            "any",
            Duration::from_secs(5),
            TEST_TAIL_ROWS,
        );
        match res {
            Err(HelperErr::PtyUnknown(id)) => assert_eq!(id, "nonexistent-pty"),
            other => panic!("expected PtyUnknown, got {:?}", other),
        }
        // Must short-circuit, not wait for the 5s timeout.
        assert!(
            start.elapsed() < Duration::from_millis(50),
            "expected immediate return, took {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn wait_for_submit_shell_succeeds_when_new_prompt_appears() {
        let h = handle();
        feed(&h, "p1", b"user@host:~$ ls -la");
        let hash_before = snapshot_screen_hash(&h, "p1").unwrap();
        let h2 = h.clone();
        let join = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(40));
            feed(&h2, "p1", b"\nfile_a\nfile_b\nuser@host:/tmp$ ");
        });
        let res = wait_for_submit_shell(&h, "p1", hash_before, Duration::from_secs(1), TEST_TAIL_ROWS);
        join.join().unwrap();
        assert!(res.is_ok(), "expected new prompt to be observed");
    }

    #[test]
    fn wait_for_submit_shell_times_out_when_prompt_unchanged() {
        let h = handle();
        feed(&h, "p1", b"user@host:~$ ls -la");
        let hash_before = snapshot_screen_hash(&h, "p1").unwrap();
        let start = std::time::Instant::now();
        let res = wait_for_submit_shell(
            &h,
            "p1",
            hash_before,
            Duration::from_millis(120),
            TEST_TAIL_ROWS,
        );
        assert!(matches!(res, Err(HelperErr::Timeout(_))), "got {:?}", res);
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(110),
            "elapsed {:?}",
            elapsed
        );
        assert!(
            elapsed < Duration::from_millis(220),
            "elapsed {:?} too long — deadline check might be broken",
            elapsed
        );
    }

    #[test]
    fn wait_for_submit_tui_drained_succeeds_when_needle_disappears() {
        let h = handle();
        feed(&h, "p1", b"prompt> hello world");
        let h2 = h.clone();
        let join = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(40));
            // Wipe the screen so the needle is no longer present.
            feed(&h2, "p1", b"\x1b[2J\x1b[Hresult line\nprompt> ");
        });
        let res = wait_for_submit_tui_drained(
            &h,
            "p1",
            "hello world",
            Duration::from_secs(2),
            Duration::from_millis(100),
            TEST_TAIL_ROWS,
        );
        join.join().unwrap();
        assert!(res.is_ok(), "expected drained needle to settle");
    }

    #[test]
    fn wait_for_submit_tui_drained_times_out_when_needle_persists() {
        let h = handle();
        feed(&h, "p1", b"prompt> hello world");
        let start = std::time::Instant::now();
        let res = wait_for_submit_tui_drained(
            &h,
            "p1",
            "hello world",
            Duration::from_millis(200),
            Duration::from_millis(50),
            TEST_TAIL_ROWS,
        );
        assert!(matches!(res, Err(HelperErr::Timeout(_))), "got {:?}", res);
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(190),
            "elapsed {:?}",
            elapsed
        );
        assert!(
            elapsed < Duration::from_millis(300),
            "elapsed {:?} too long — deadline check might be broken",
            elapsed
        );
    }
}
