use std::collections::VecDeque;

const STDERR_LIMIT: usize = 8192;

#[derive(Default)]
pub(super) struct Output {
    stderr: VecDeque<u8>,
    pub exit: Option<String>,
}

impl Output {
    pub fn append_stderr(&mut self, bytes: &[u8]) {
        let bytes = &bytes[bytes.len().saturating_sub(STDERR_LIMIT)..];
        let retained = STDERR_LIMIT - bytes.len();
        self.stderr
            .drain(..self.stderr.len().saturating_sub(retained));
        self.stderr.extend(bytes);
    }

    pub fn failure(&self, reason: &str, pid: u32) -> String {
        let stderr = self.stderr.iter().copied().collect::<Vec<_>>();
        format!(
            "{reason}: pid={pid} exit={} stderr={:?}",
            self.exit.as_deref().unwrap_or("not observed"),
            String::from_utf8_lossy(&stderr),
        )
    }

    pub fn listening(&self, expected: &str) -> bool {
        let bytes = self.stderr.iter().copied().collect::<Vec<_>>();
        !expected.is_empty()
            && bytes
                .windows(expected.len())
                .any(|window| window == expected.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_output_retains_bounded_latest_stderr() {
        let mut output = Output::default();
        output.append_stderr(&vec![b'a'; STDERR_LIMIT]);
        output.append_stderr(b"last");
        assert_eq!(output.stderr.len(), STDERR_LIMIT);
        assert!(output.failure("failed", 42).contains("last"));
        output.append_stderr(&vec![b'b'; STDERR_LIMIT * 2]);
        assert_eq!(output.stderr.len(), STDERR_LIMIT);
        assert!(output.stderr.iter().all(|byte| *byte == b'b'));
    }

    #[test]
    fn startup_output_reports_exit_and_preserves_invalid_utf8_evidence() {
        let mut output = Output::default();
        output.exit = Some("exit status: 7".into());
        output.append_stderr(b"provider failed\xff");
        let failure = output.failure("before listening", 42);
        assert!(failure.contains("pid=42"));
        assert!(failure.contains("exit status: 7"));
        assert!(failure.contains("provider failed"));
        assert!(failure.contains('\u{fffd}'));
    }

    #[test]
    fn startup_output_never_invents_exit_evidence() {
        assert!(Output::default()
            .failure("deadline", 42)
            .contains("exit=not observed"));
    }

    #[test]
    fn startup_output_detects_listening_across_read_boundaries() {
        let mut output = Output::default();
        output.append_stderr(b"\xfflistening on: ws://127.");
        assert!(!output.listening("listening on: ws://127.0.0.1:42"));
        output.append_stderr(b"0.0.1:42\n");
        assert!(output.listening("listening on: ws://127.0.0.1:42"));
        assert!(!output.listening("listening on: ws://127.0.0.1:43"));
        assert!(!output.listening(""));
    }
}
