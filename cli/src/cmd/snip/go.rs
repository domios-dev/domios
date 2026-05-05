//! Go test output truncator.
//!
//! Keep:
//!   - `--- FAIL:` test markers
//!   - `FAIL` summary lines
//!   - `=== RUN` (gives test counts)
//!   - panic blocks (until next blank line or `--- ` divider)
//!   - `t.Log` / failing assertion bodies after a `--- FAIL:` until next `---` or `=== `
//!   - Coverage / build error lines
//!
//! Drop:
//!   - `--- PASS:` markers
//!   - `=== PAUSE` / `=== CONT`
//!   - `ok\tpkg/name\t0.123s` — package-level pass summary
//!   - `?\tpkg/name\t[no test files]`
//!
//! Unknown subcommand → identity passthrough.

const RELEVANT_SUBCMDS: &[&str] = &["test", "vet", "build"];

pub fn snip(args: &[String], input: &str) -> String {
    let sub = match first_non_flag(args) {
        Some(s) => s,
        None => return input.to_string(),
    };
    if !RELEVANT_SUBCMDS.contains(&sub) {
        return input.to_string();
    }
    snip_lines(input)
}

fn first_non_flag(args: &[String]) -> Option<&str> {
    for a in args {
        if !a.starts_with('-') {
            return Some(a.as_str());
        }
    }
    None
}

#[derive(PartialEq, Eq)]
enum Block {
    None,
    Panic,
    FailBody,
}

pub(crate) fn snip_lines(input: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut block = Block::None;

    for line in input.lines() {
        if matches!(block, Block::Panic) {
            out.push(line);
            if line.is_empty() {
                block = Block::None;
            }
            continue;
        }
        if matches!(block, Block::FailBody) {
            if line.starts_with("=== ") || line.starts_with("--- ") || line.starts_with("FAIL") || line.starts_with("ok\t") || line.is_empty() {
                block = Block::None;
            } else {
                out.push(line);
                continue;
            }
        }

        if line.starts_with("--- FAIL:") {
            out.push(line);
            block = Block::FailBody;
            continue;
        }

        if line.starts_with("--- PASS:") {
            continue;
        }

        if line.starts_with("=== PAUSE") || line.starts_with("=== CONT") {
            continue;
        }

        if is_panic_start(line) {
            out.push(line);
            block = Block::Panic;
            continue;
        }

        if is_pkg_pass_line(line) {
            continue;
        }

        if is_pkg_no_tests_line(line) {
            continue;
        }

        out.push(line);
    }

    join_with_newline(&out, input)
}

fn is_panic_start(line: &str) -> bool {
    line.contains("panic:") || line.starts_with("goroutine ") || line.starts_with("fatal error:")
}

/// Lines like `ok  \tpkg/name\t0.123s` or with `[no tests to run]`.
fn is_pkg_pass_line(line: &str) -> bool {
    if !line.starts_with("ok\t") && !line.starts_with("ok  ") {
        return false;
    }
    line.ends_with('s')
        || line.ends_with("[no tests to run]")
        || line.contains("[no tests to run]")
        || line.contains("(cached)")
}

fn is_pkg_no_tests_line(line: &str) -> bool {
    line.starts_with("?\t") && line.contains("[no test files]")
}

fn join_with_newline(lines: &[&str], original: &str) -> String {
    let mut s = lines.join("\n");
    if original.ends_with('\n') && !lines.is_empty() {
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> String {
        v.to_string()
    }

    #[test]
    fn passthrough_for_unknown_subcommand() {
        let input = "ok\tpkg/x\t0.1s";
        assert_eq!(snip(&[s("run")], input), input);
    }

    #[test]
    fn passthrough_when_no_subcommand() {
        let input = "ok\tpkg/x\t0.1s";
        assert_eq!(snip(&[], input), input);
    }

    #[test]
    fn drops_pass_marker() {
        let input = "--- PASS: TestFoo (0.01s)";
        assert_eq!(snip(&[s("test")], input), "");
    }

    #[test]
    fn keeps_fail_marker() {
        let input = "--- FAIL: TestFoo (0.01s)";
        let out = snip(&[s("test")], input);
        assert!(out.contains("--- FAIL: TestFoo"));
    }

    #[test]
    fn drops_pause() {
        let input = "=== PAUSE TestFoo";
        assert_eq!(snip(&[s("test")], input), "");
    }

    #[test]
    fn drops_cont() {
        let input = "=== CONT TestFoo";
        assert_eq!(snip(&[s("test")], input), "");
    }

    #[test]
    fn keeps_run() {
        let input = "=== RUN TestFoo";
        let out = snip(&[s("test")], input);
        assert!(out.contains("=== RUN TestFoo"));
    }

    #[test]
    fn drops_pkg_ok_line() {
        let input = "ok\tpkg/foo\t0.123s";
        assert_eq!(snip(&[s("test")], input), "");
    }

    #[test]
    fn drops_pkg_ok_cached() {
        let input = "ok\tpkg/foo\t(cached)";
        assert_eq!(snip(&[s("test")], input), "");
    }

    #[test]
    fn drops_pkg_no_tests() {
        let input = "?\tpkg/foo\t[no test files]";
        assert_eq!(snip(&[s("test")], input), "");
    }

    #[test]
    fn keeps_fail_summary() {
        let input = "FAIL\tpkg/foo\t0.123s";
        let out = snip(&[s("test")], input);
        assert!(out.contains("FAIL"));
    }

    #[test]
    fn keeps_panic_block_until_blank_line() {
        let input = "panic: oh no\ngoroutine 1 [running]:\nmain.foo()\n\nafter blank";
        let out = snip(&[s("test")], input);
        assert!(out.contains("panic: oh no"));
        assert!(out.contains("goroutine 1"));
        assert!(out.contains("main.foo()"));
    }

    #[test]
    fn keeps_fatal_error_block() {
        let input = "fatal error: stack overflow\nstack trace line";
        let out = snip(&[s("test")], input);
        assert!(out.contains("fatal error"));
        assert!(out.contains("stack trace"));
    }

    #[test]
    fn keeps_fail_body() {
        let input = "--- FAIL: TestFoo (0.01s)\n    foo_test.go:10: expected 1, got 2\n=== RUN TestBar";
        let out = snip(&[s("test")], input);
        assert!(out.contains("expected 1, got 2"));
        assert!(out.contains("=== RUN TestBar"));
    }

    #[test]
    fn green_run_high_reduction() {
        let mut input = String::new();
        for i in 0..100 {
            input.push_str(&format!("=== RUN TestFoo{}\n", i));
            input.push_str(&format!("--- PASS: TestFoo{} (0.00s)\n", i));
        }
        input.push_str("PASS\nok\tpkg/x\t0.123s\n");
        let out = snip(&[s("test")], &input);
        let reduction = 1.0 - (out.len() as f64 / input.len() as f64);
        assert!(reduction > 0.5, "expected >50% reduction, got {}", reduction);
        assert!(!out.contains("PASS:"));
        assert!(!out.contains("ok\tpkg/x"));
    }

    #[test]
    fn all_fail_preserves_diagnostics() {
        let input = "=== RUN TestFoo\n--- FAIL: TestFoo (0.00s)\n    foo_test.go:5: assertion failed\nFAIL\nFAIL\tpkg/x\t0.001s";
        let out = snip(&[s("test")], input);
        assert!(out.contains("--- FAIL: TestFoo"));
        assert!(out.contains("assertion failed"));
        assert!(out.contains("FAIL"));
    }

    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(snip(&[s("test")], ""), "");
    }

    #[test]
    fn preserves_trailing_newline() {
        let input = "FAIL\n";
        assert_eq!(snip(&[s("test")], input), "FAIL\n");
    }
}
