use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;

use crate::pty::screen_state::screen_has_shell_prompt;
use crate::ScreenStateHandle;

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
    /// The submit needed the bounded recovery path after paste or turn evidence
    /// was missed. Success still required the provider turn transition.
    pub submit_anomaly: bool,
    pub elapsed: Duration,
}

impl InjectionResult {
    pub fn require_submission(
        self,
        required: bool,
        waited: Duration,
        screen_tail: String,
    ) -> Result<Self, InjectionError> {
        if required && !self.submit_observed {
            return Err(InjectionError::SubmitNeverConfirmed {
                waited,
                screen_tail,
            });
        }
        Ok(self)
    }
}

#[derive(Debug, Clone)]
pub enum InjectionError {
    PromptNeverReady {
        waited: Duration,
    },
    EchoMismatch {
        expected_substr: String,
        screen_tail: String,
    },
    SubmitNeverConfirmed {
        waited: Duration,
        screen_tail: String,
    },
    PtyClosed {
        pty_id: String,
    },
    AgentInputUnavailable {
        pty_id: String,
    },
    AgentTuiPresent {
        pty_id: String,
    },
}

impl std::fmt::Display for InjectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PromptNeverReady { waited } => {
                write!(f, "prompt never became ready after {:?}", waited)
            }
            Self::EchoMismatch {
                expected_substr,
                screen_tail,
            } => {
                write!(
                    f,
                    "expected echo not found on screen; expected substring={:?}; screen_tail={:?}",
                    expected_substr, screen_tail
                )
            }
            Self::SubmitNeverConfirmed {
                waited,
                screen_tail,
            } => {
                write!(
                    f,
                    "submit not confirmed after {:?}; screen_tail={:?}",
                    waited, screen_tail
                )
            }
            Self::PtyClosed { pty_id } => write!(f, "pty {} is closed", pty_id),
            Self::AgentInputUnavailable { pty_id } => {
                write!(
                    f,
                    "agent_unavailable: input permit is stale for pty {}",
                    pty_id
                )
            }
            Self::AgentTuiPresent { pty_id } => {
                write!(
                    f,
                    "agent TUI present on pty {} - launch write refused",
                    pty_id
                )
            }
        }
    }
}

impl std::error::Error for InjectionError {}

#[derive(Debug, Clone)]
pub enum HelperErr {
    Timeout(String),    // screen tail captured at timeout
    PtyUnknown(String), // pty_id with no bound parser
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_ws_with_line_boundaries(s: &str) -> (String, Vec<usize>) {
    let mut normalized = String::new();
    let mut boundaries = Vec::new();
    let mut previous_line = None;

    for (line_index, line) in s.lines().enumerate() {
        for word in line.split_whitespace() {
            if !normalized.is_empty() {
                if previous_line != Some(line_index) {
                    boundaries.push(normalized.chars().count());
                }
                normalized.push(' ');
            }
            normalized.push_str(word);
            previous_line = Some(line_index);
        }
    }

    (normalized, boundaries)
}

fn soft_wrap_elision_matches(screen_tail: &str, expected: &str) -> bool {
    let (tail, boundaries) = normalize_ws_with_line_boundaries(screen_tail);
    let tail: Vec<char> = tail.chars().collect();
    let expected: Vec<char> = normalize_ws(expected).chars().collect();

    for boundary in boundaries {
        for omitted in 0..expected.len().saturating_sub(1) {
            if expected[omitted].is_whitespace() || !expected[omitted + 1].is_whitespace() {
                continue;
            }
            let Some(start) = boundary.checked_sub(omitted) else {
                continue;
            };
            if start + expected.len().saturating_sub(1) > tail.len() {
                continue;
            }

            let matches = expected
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != omitted)
                .enumerate()
                .all(|(tail_offset, (_, expected_char))| {
                    tail[start + tail_offset] == *expected_char
                });
            if matches {
                return true;
            }
        }
    }

    false
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
    RE.get_or_init(|| {
        Regex::new(r"\[(?:Pasted text #\d+( \+\d+ lines?)?|Pasted Content \d+ chars(?: #\d+)?)\]")
            .unwrap()
    })
}

#[cfg(test)]
fn screen_has_paste_placeholder(screen: &str) -> bool {
    paste_placeholder_regex().is_match(screen)
}

fn match_count(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 1;
    }
    haystack.match_indices(needle).count()
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
    screen_echo_evidence_count(screen_tail, expected) > 0
}

pub fn screen_echo_evidence_count(screen_tail: &str, expected: &str) -> usize {
    if expected.is_empty() {
        return 1;
    }
    let n_expected = normalize_ws(expected);
    let n_tail = normalize_ws(screen_tail);
    let mut evidence = match_count(&n_tail, &n_expected);
    let flat = screen_tail.replace('\n', "");
    let n_flat = normalize_ws(&flat);
    evidence = evidence.max(match_count(&n_flat, &n_expected));
    if soft_wrap_elision_matches(screen_tail, expected) {
        evidence = evidence.max(1);
    }
    // paste still is — match the last N chars instead. Gated on input size so a ~90-char command with a corrupted prefix is still caught by the strict substring check above.
    if expected.len() >= ECHO_SUFFIX_MATCH_MIN_INPUT_BYTES {
        let suffix = last_n_chars(&n_expected, ECHO_SUFFIX_MATCH_CHARS);
        evidence = evidence.max(match_count(&n_tail, &suffix));
        evidence = evidence.max(match_count(&n_flat, &suffix));
    }
    // the paste was accepted. Treat the placeholder as echo confirmation, but only for inputs that would trigger the placeholder rendering.
    let placeholder_eligible = expected.contains('\n') || expected.len() > ECHO_LONG_INPUT_BYTES;
    if placeholder_eligible {
        evidence = evidence.max(paste_placeholder_regex().find_iter(screen_tail).count());
    }
    evidence
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
    let signal = screen_state.lock().arm_screen_change_signal(pty_id);
    let deadline = std::time::Instant::now() + timeout;
    let mut generation = signal.generation();
    loop {
        let (tail_opt, hash_now) = {
            let s = screen_state.lock();
            (
                s.tail_text(pty_id, tail_rows),
                s.get_content_hash(pty_id, 0),
            )
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
        let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
            return Err(HelperErr::Timeout(tail));
        };
        if !signal.wait_after(generation, remaining) {
            return Err(HelperErr::Timeout(tail));
        }
        generation = signal.generation();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock_hold::TimedMutex;
    use crate::pty::screen_state::ScreenState;
    use std::sync::Arc;

    fn handle() -> ScreenStateHandle {
        Arc::new(TimedMutex::new("screen_state", ScreenState::new()))
    }

    fn feed(h: &ScreenStateHandle, pty: &str, bytes: &[u8]) {
        h.lock().process(pty, bytes);
    }

    #[test]
    fn screen_contains_echo_handles_simple_match() {
        assert!(screen_contains_echo("user@host:~$ ls -la", "ls -la"));
    }

    #[test]
    fn required_submit_cannot_succeed_unobserved() {
        let result = InjectionResult {
            bytes_written: 10,
            echo_observed: true,
            submit_observed: false,
            submit_anomaly: false,
            elapsed: Duration::ZERO,
        };
        assert!(matches!(
            result.require_submission(true, Duration::ZERO, "> parked".into()),
            Err(InjectionError::SubmitNeverConfirmed { .. })
        ));
    }

    #[test]
    fn optimistic_submit_keeps_unconfirmed_ack_as_diagnostic() {
        let result = InjectionResult {
            bytes_written: 10,
            echo_observed: true,
            submit_observed: false,
            submit_anomaly: true,
            elapsed: Duration::ZERO,
        };
        let accepted = result
            .require_submission(false, Duration::ZERO, "> provider working".into())
            .expect("an acknowledgement miss must not reject a production send");
        assert!(accepted.submit_anomaly);
        assert!(!accepted.submit_observed);
    }

    #[test]
    fn screen_contains_echo_normalizes_whitespace() {
        assert!(screen_contains_echo("user@host:~$  ls    -la\n", "ls -la",));
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
        let screen = "user@host codeterm % nset CLAUDECODE && claude --dangerously-skip-permissions \"$(cat /tmp/.ct-task-01219c41)\")\"";
        let expected = r#"unset CLAUDECODE && claude --dangerously-skip-permissions "$(cat /tmp/.ct-task-01219c41)""#;
        assert!(!screen_contains_echo(screen, expected));
    }

    #[test]
    fn wrap_boundary_elision_fixture_matches_only_the_lost_wrap_character() {
        // Literal sub-defect 3 fixture from sec_gnk2xm: the composer rendered
        // `liv` at a soft wrap where the sent report contained `live`.
        let expected = "status+body; live payloads confirm";
        let captured_tail = "status+body; liv\npayloads confirm";
        assert!(screen_contains_echo(captured_tail, expected));
    }

    #[test]
    fn tail_text_preserves_the_wrap_column_character() {
        let h = handle();
        {
            let mut state = h.lock();
            state.resize("p1", 16, 50);
            state.process("p1", b"status+body; live payloads confirm");
        }
        let tail = h.lock().tail_text("p1", TEST_TAIL_ROWS).unwrap();
        assert!(
            tail.replace('\n', "").contains("status+body; live"),
            "tail_text must retain the final character before the wrap: {tail:?}"
        );
    }

    #[test]
    fn duplicated_trailing_paren_passes_echo_check() {
        // Substring still matches with duplicated `)"` tail; submit verify catches it.
        let screen = "user@host codeterm % unset CLAUDECODE && claude --dangerously-skip-permissions \"$(cat /tmp/.ct-task-01219c41)\")\"";
        let expected = r#"unset CLAUDECODE && claude --dangerously-skip-permissions "$(cat /tmp/.ct-task-01219c41)""#;
        assert!(screen_contains_echo(screen, expected));
    }

    #[test]
    fn long_paste_with_scrolled_out_prefix_matches_via_suffix() {
        // 5000-char paste: only the trailing portion fits into the 8-row tail window. Echo verify must still confirm via suffix match.
        let body: String = "A".repeat(4900);
        let expected = format!(
            "-=-codeterm:mesh peer=peer-a pane=abc12345-=-{} TAIL_TOKEN_END",
            body
        );
        // Tail captured by screen_state contains only the last ~640 chars of the paste.
        let visible_tail: String = expected
            .chars()
            .rev()
            .take(600)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        assert!(
            screen_contains_echo(&visible_tail, &expected),
            "long paste should match via suffix even when prefix scrolled out"
        );
    }

    #[test]
    fn bracketed_paste_placeholder_counts_as_echo_for_multiline_input() {
        // Claude TUI replaces multi-line pastes with the placeholder; literal text never appears on screen.
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
        let expected = format!(
            "-=-codeterm:mesh peer=PEER pane=abc12345-=-{}",
            "x".repeat(500)
        );
        assert!(
            screen_contains_echo(screen, &expected),
            "long single-line paste replaced by placeholder must count as echo"
        );
    }

    #[test]
    fn bracketed_paste_placeholder_without_lines_suffix_counts_as_echo() {
        // required the suffix and missed this case entirely, which is why the v1.1.18 smoke test failed for ~4.7KB single-line mesh messages.
        let screen = "> [Pasted text #1]";
        let expected = format!(
            "-=-codeterm:mesh peer=PEER pane=abc12345-=-{}",
            "x".repeat(4500)
        );
        assert!(
            screen_contains_echo(screen, &expected),
            "single-line paste without `+N lines` suffix must still count as echo"
        );
    }

    #[test]
    fn placeholder_does_not_falsely_match_short_single_line_input() {
        // Defensive: a literal "[Pasted text #N +M lines]" on screen must not confirm an unrelated short input.
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
        assert!(screen_has_paste_placeholder(
            "> [Pasted text #99 +250 lines]"
        ));
        // v1.1.19 widened the regex to also accept the bare `[Pasted text #N]` shape (without the trailing `+M lines` suffix).
        assert!(screen_has_paste_placeholder("> [Pasted text #1]"));
        // Bracket pair is required; the unbracketed prose form must not match.
        assert!(!screen_has_paste_placeholder("Pasted text #1 +1 line"));
    }

    #[test]
    fn pasted_content_placeholder_counts_as_echo_only_for_eligible_input() {
        let expected = "x".repeat(2256);
        assert!(screen_contains_echo(
            "[Pasted Content 2256 chars]",
            &expected
        ));
        assert!(screen_contains_echo(
            "[Pasted Content 2256 chars] #2",
            &expected
        ));
        assert!(!screen_contains_echo(
            "[Pasted Content 2256 chars]",
            "short input"
        ));
    }

    #[test]
    fn echo_evidence_counts_repeated_fuzzy_messages() {
        let expected = "You are the Orchestrator of this team";
        let screen =
            "You are the Orchestrator\nof this team\n\nYou are the Orchestrator of this team";
        assert_eq!(screen_echo_evidence_count(screen, expected), 2);
    }

    #[test]
    fn echo_evidence_counts_multiple_paste_placeholders() {
        let expected = "long message ".repeat(40);
        let before = "> [Pasted Content 520 chars]";
        let after = "> [Pasted Content 520 chars]\n> [Pasted text #2 +3 lines]";
        assert_eq!(screen_echo_evidence_count(before, &expected), 1);
        assert_eq!(screen_echo_evidence_count(after, &expected), 2);
    }

    // vt100 default is 50 rows; tail_rows >= 50 scans the whole screen.
    const TEST_TAIL_ROWS: usize = 60;

    #[test]
    fn wait_for_submit_shell_succeeds_when_new_prompt_appears() {
        let h = handle();
        feed(&h, "p1", b"user@host:~$ ls -la");
        let hash_before = snapshot_screen_hash(&h, "p1").unwrap();
        feed(&h, "p1", b"\nfile_a\nfile_b\nuser@host:/tmp$ ");
        let res = wait_for_submit_shell(
            &h,
            "p1",
            hash_before,
            Duration::from_secs(1),
            TEST_TAIL_ROWS,
        );
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
}
