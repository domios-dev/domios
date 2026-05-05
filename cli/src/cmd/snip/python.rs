//! Pytest output truncator.
//!
//! Keep:
//!   - Header (`platform`, `rootdir`, `collected`, `configfile`)
//!   - `^FAILED`, `^ERROR` markers
//!   - `=== FAILURES ===` section, full traceback for first 3 failures
//!   - `=== short test summary info ===` block
//!   - Final summary `=== N failed, M passed in T s ===`
//!
//! Drop:
//!   - `^PASSED` / `^SKIPPED` / `^XPASS` / `^XFAIL` per-test lines
//!   - Single-character row mode (`....F.s..` etc) — keep only if mixed.
//!     We keep these as-is for now (low cost) and only drop the obvious
//!     PASSED bloat.

pub fn snip(head: &str, args: &[String], input: &str) -> String {
    if !is_pytest_invocation(head, args) {
        return input.to_string();
    }
    snip_lines(input)
}

fn is_pytest_invocation(head: &str, args: &[String]) -> bool {
    if head == "pytest" || head == "py.test" {
        return true;
    }
    if head == "python" || head == "python3" {
        return args.iter().any(|a| a == "pytest" || a == "-m" && false)
            || args.windows(2).any(|w| w[0] == "-m" && (w[1] == "pytest" || w[1] == "py.test"));
    }
    false
}

const MAX_FAILURES: usize = 3;

pub(crate) fn snip_lines(input: &str) -> String {
    let lines: Vec<&str> = input.lines().collect();
    let trailing_nl = input.ends_with('\n');
    let mut out: Vec<String> = Vec::with_capacity(lines.len());

    let mut in_failures_section = false;
    let mut in_short_summary = false;
    let mut failures_seen = 0usize;
    let mut keeping_current_failure = true;

    for line in lines.iter() {
        if is_section_header(line, "FAILURES") {
            in_failures_section = true;
            in_short_summary = false;
            failures_seen = 0;
            keeping_current_failure = true;
            out.push((*line).to_string());
            continue;
        }
        if is_section_header(line, "ERRORS") {
            in_failures_section = true;
            in_short_summary = false;
            keeping_current_failure = true;
            out.push((*line).to_string());
            continue;
        }
        if is_section_header(line, "short test summary info") {
            in_failures_section = false;
            in_short_summary = true;
            out.push((*line).to_string());
            continue;
        }
        if is_section_header(line, "=") || (line.starts_with("===") && line.ends_with("===")) {
            in_failures_section = false;
            in_short_summary = false;
            out.push((*line).to_string());
            continue;
        }

        if in_failures_section {
            if is_failure_block_divider(line) {
                failures_seen += 1;
                keeping_current_failure = failures_seen <= MAX_FAILURES;
            }
            if keeping_current_failure {
                out.push((*line).to_string());
            } else if failures_seen == MAX_FAILURES + 1 {
                out.push(format!(
                    "... (additional failure tracebacks omitted; see short test summary info) ..."
                ));
                keeping_current_failure = false;
            }
            continue;
        }

        if in_short_summary {
            out.push((*line).to_string());
            continue;
        }

        if is_drop_line(line) {
            continue;
        }
        out.push((*line).to_string());
    }

    let mut s = out.join("\n");
    if trailing_nl && !out.is_empty() {
        s.push('\n');
    }
    s
}

fn is_section_header(line: &str, marker: &str) -> bool {
    if marker == "=" {
        return false;
    }
    let trimmed = line.trim_start_matches('=').trim();
    trimmed.starts_with(marker)
        || trimmed.contains(&format!(" {} ", marker))
        || trimmed == marker
}

fn is_failure_block_divider(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    let underscores = t.chars().filter(|c| *c == '_').count();
    underscores >= 5 && t.len() >= 10 && t.chars().all(|c| c == '_' || c == ' ' || c.is_alphanumeric() || matches!(c, '.' | ':' | '[' | ']' | '-' | '/'))
        && t.starts_with('_')
        && t.ends_with('_')
}

fn is_drop_line(line: &str) -> bool {
    let t = line.trim_start();
    if t.is_empty() {
        return false;
    }
    if t.contains(" PASSED ") || t.ends_with(" PASSED") {
        return true;
    }
    if t.contains(" SKIPPED ") || t.ends_with(" SKIPPED") {
        return true;
    }
    if t.contains(" XPASS ") || t.ends_with(" XPASS") {
        return true;
    }
    if t.contains(" XFAIL ") || t.ends_with(" XFAIL") {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> String {
        v.to_string()
    }

    #[test]
    fn pytest_bare_recognised() {
        let out = snip("pytest", &[], "tests/foo.py::test_a PASSED                       [ 50%]");
        assert_eq!(out, "");
    }

    #[test]
    fn py_test_recognised() {
        let out = snip("py.test", &[], "tests/foo.py::test_a PASSED                       [ 50%]");
        assert_eq!(out, "");
    }

    #[test]
    fn python_m_pytest_recognised() {
        let out = snip(
            "python",
            &[s("-m"), s("pytest")],
            "tests/foo.py::test_a PASSED                       [ 50%]",
        );
        assert_eq!(out, "");
    }

    #[test]
    fn python3_m_pytest_recognised() {
        let out = snip(
            "python3",
            &[s("-m"), s("pytest")],
            "tests/foo.py::test_a PASSED                       [ 50%]",
        );
        assert_eq!(out, "");
    }

    #[test]
    fn unknown_python_passthrough() {
        let input = "anything";
        assert_eq!(snip("python", &[s("script.py")], input), input);
    }

    #[test]
    fn unknown_head_passthrough() {
        let input = "anything";
        assert_eq!(snip("ruby", &[], input), input);
    }

    #[test]
    fn drops_passed_lines() {
        let input = "tests/foo.py::test_a PASSED [ 50%]\ntests/foo.py::test_b PASSED [100%]";
        assert_eq!(snip("pytest", &[], input), "");
    }

    #[test]
    fn drops_skipped_lines() {
        let input = "tests/foo.py::test_a SKIPPED [ 50%]";
        assert_eq!(snip("pytest", &[], input), "");
    }

    #[test]
    fn drops_xpass_xfail() {
        let input = "tests/foo.py::test_a XPASS [ 50%]\ntests/foo.py::test_b XFAIL [100%]";
        assert_eq!(snip("pytest", &[], input), "");
    }

    #[test]
    fn keeps_failed_line() {
        let input = "FAILED tests/foo.py::test_a - assert 1 == 2";
        let out = snip("pytest", &[], input);
        assert!(out.contains("FAILED"));
    }

    #[test]
    fn keeps_error_line() {
        let input = "ERROR tests/foo.py::test_setup - fixture not found";
        let out = snip("pytest", &[], input);
        assert!(out.contains("ERROR"));
    }

    #[test]
    fn keeps_section_headers() {
        let input = "============================= test session starts ==============================\nplatform linux\ncollected 5 items\n\ntests/foo.py::test_a PASSED                                  [ 20%]\n=================================== FAILURES ===================================\n____________________ test_b ____________________\n\n    def test_b():\n>       assert False\nE       assert False\n\n=========================== short test summary info ============================\nFAILED tests/foo.py::test_b\n=========================== 1 failed, 1 passed in 0.05s ============================";
        let out = snip("pytest", &[], input);
        assert!(out.contains("test session starts"));
        assert!(out.contains("FAILURES"));
        assert!(out.contains("short test summary"));
        assert!(out.contains("1 failed, 1 passed"));
        assert!(!out.contains("test_a PASSED"));
        assert!(out.contains("FAILED tests/foo.py::test_b"));
    }

    #[test]
    fn keeps_failure_traceback() {
        let input = "=================================== FAILURES ===================================\n____________________ test_b ____________________\n\n    def test_b():\n>       assert False\nE       assert False\n\ntests/foo.py:5: AssertionError";
        let out = snip("pytest", &[], input);
        assert!(out.contains("def test_b()"));
        assert!(out.contains("AssertionError"));
    }

    #[test]
    fn caps_failure_blocks_at_three() {
        let mut input = String::from("=================================== FAILURES ===================================\n");
        for i in 0..6 {
            input.push_str(&format!("____________________ test_{} ____________________\n", i));
            input.push_str(&format!("    body for test_{}\n", i));
            input.push_str(&format!("E    failure body line {}\n", i));
        }
        let out = snip("pytest", &[], &input);
        assert!(out.contains("test_0"));
        assert!(out.contains("test_1"));
        assert!(out.contains("test_2"));
        assert!(out.contains("omitted"));
        assert!(!out.contains("body for test_4"));
    }

    #[test]
    fn green_run_high_reduction() {
        let mut input = String::from("============================= test session starts ==============================\nplatform linux -- Python 3.10\ncollected 100 items\n\n");
        for i in 0..100 {
            input.push_str(&format!("tests/foo.py::test_{} PASSED                                  [{:>3}%]\n", i, i));
        }
        input.push_str("\n=========================== 100 passed in 0.05s ============================");
        let out = snip("pytest", &[], &input);
        let reduction = 1.0 - (out.len() as f64 / input.len() as f64);
        assert!(reduction > 0.7, "expected >70% reduction, got {}", reduction);
        assert!(out.contains("100 passed"));
        assert!(out.contains("test session starts"));
    }

    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(snip("pytest", &[], ""), "");
    }

    #[test]
    fn keeps_collected_count() {
        let input = "collected 5 items";
        let out = snip("pytest", &[], input);
        assert!(out.contains("collected 5 items"));
    }

    #[test]
    fn keeps_platform_header() {
        let input = "platform linux -- Python 3.10";
        let out = snip("pytest", &[], input);
        assert!(out.contains("platform linux"));
    }
}
