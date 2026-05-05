//! Cargo output truncator. Targets `cargo test` and `cargo build`.
//!
//! Keep:
//!   - `^test result:` summary lines
//!   - `FAILED` markers and failing-test names
//!   - `---- <name> stdout ----` block headers + the next 20 lines (the
//!     actual panic / assert message)
//!   - `error[E…]` / `error: …` blocks
//!   - First 5 `warning:` / `warning[…]:` lines, drop the rest
//!   - `running N tests` headers (gives green-pass test counts)
//!   - `failures:` section header + the bare names that follow
//!
//! Drop:
//!   - `Compiling …`, `Finished …`, `Documenting …`, `Updating …`,
//!     `Locking …`, `Downloaded …`, `Downloading …`
//!   - `Running unittests…` and `Running …`  (cargo prints one per binary)
//!   - `test … ok` / `test … ignored` (the per-test green lines)
//!   - The `Doc-tests` section banner
//!   - Blank lines between dropped runs
//!
//! Unknown subcommand (`cargo run`, `cargo fmt`) → identity passthrough.

const RELEVANT_SUBCMDS: &[&str] = &[
    "test", "t",          // alias for test in some configs; harmless
    "build", "b",
    "check", "c",
    "clippy",
    "doc",
    "bench",
];

/// Run truncation if the cargo subcommand is one we have patterns for.
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
        if !a.starts_with('-') && !a.starts_with('+') {
            return Some(a.as_str());
        }
    }
    None
}

const FAILURE_BLOCK_LINES: usize = 20;
const MAX_WARNINGS: usize = 5;

pub(crate) fn snip_lines(input: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut warnings_kept = 0usize;
    let mut in_failure_block = false;
    let mut failure_block_seen = 0usize;
    let mut in_error_block = false;

    for line in input.lines() {
        if in_failure_block {
            if line.starts_with("---- ") && line.ends_with(" ----") {
                in_failure_block = false;
            } else if failure_block_seen < FAILURE_BLOCK_LINES {
                out.push(line);
                failure_block_seen += 1;
                continue;
            } else {
                in_failure_block = false;
            }
        }

        if in_error_block {
            if line.is_empty() || line.starts_with("error") || line.starts_with("warning") || !line.starts_with(' ') {
                in_error_block = false;
            } else {
                out.push(line);
                continue;
            }
        }

        if is_failure_block_header(line) {
            out.push(line);
            in_failure_block = true;
            failure_block_seen = 0;
            continue;
        }

        if is_error_line(line) {
            out.push(line);
            in_error_block = true;
            continue;
        }

        if is_warning_line(line) {
            if warnings_kept < MAX_WARNINGS {
                out.push(line);
                warnings_kept += 1;
            }
            continue;
        }

        if is_drop_line(line) {
            continue;
        }

        if is_keep_line(line) {
            out.push(line);
            continue;
        }
    }

    join_with_newline(&out, input)
}

fn is_failure_block_header(line: &str) -> bool {
    line.starts_with("---- ")
        && (line.ends_with(" stdout ----") || line.ends_with(" stderr ----"))
}

fn is_error_line(line: &str) -> bool {
    line.starts_with("error[")
        || line.starts_with("error:")
        || line.starts_with("error ")
        || line == "error"
}

fn is_warning_line(line: &str) -> bool {
    line.starts_with("warning:") || line.starts_with("warning[")
}

fn is_drop_line(line: &str) -> bool {
    let t = line.trim_start();
    if t.starts_with("Compiling ")
        || t.starts_with("Finished ")
        || t.starts_with("Documenting ")
        || t.starts_with("Running ")
        || t.starts_with("Locking ")
        || t.starts_with("Updating ")
        || t.starts_with("Downloaded ")
        || t.starts_with("Downloading ")
        || t.starts_with("Fetching ")
        || t.starts_with("Doc-tests ")
        || t.starts_with("Installing ")
    {
        return true;
    }
    if line.starts_with("test ") {
        if line.ends_with(" ... ok")
            || line.ends_with(" ... ignored")
            || line.contains(" ... ok ") // bench reports include trailing data
        {
            return true;
        }
    }
    false
}

fn is_keep_line(line: &str) -> bool {
    if line.starts_with("test result:")
        || line.contains("FAILED")
        || line.starts_with("running ")
        || line == "failures:"
        || line.starts_with("failures:")
        || line.starts_with("note:")
        || line.starts_with("help:")
        || line.starts_with("  --> ")
        || line.starts_with("  = ")
    {
        return true;
    }
    if line.starts_with("    ") && !line.trim().is_empty() {
        return true;
    }
    false
}

fn join_with_newline(lines: &[&str], original: &str) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut s = lines.join("\n");
    if original.ends_with('\n') {
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
        let input = "Compiling foo v0.1.0\nFinished `dev` profile";
        assert_eq!(snip(&[s("run")], input), input);
    }

    #[test]
    fn passthrough_when_no_subcommand() {
        let input = "Compiling foo v0.1.0";
        assert_eq!(snip(&[], input), input);
    }

    #[test]
    fn passthrough_when_subcommand_after_flags_unknown() {
        let input = "Compiling foo";
        assert_eq!(snip(&[s("--quiet"), s("run")], input), input);
    }

    #[test]
    fn skips_leading_flags_to_find_subcommand() {
        let input = "Compiling foo v0.1.0\nFinished `dev` profile\ntest result: ok. 0 passed";
        let out = snip(&[s("--quiet"), s("test")], input);
        assert!(!out.contains("Compiling"));
        assert!(out.contains("test result"));
    }

    #[test]
    fn drops_compiling_lines() {
        let input = "Compiling foo v0.1.0\ntest result: ok. 0 passed";
        let out = snip(&[s("test")], input);
        assert!(!out.contains("Compiling"));
        assert!(out.contains("test result"));
    }

    #[test]
    fn drops_finished_lines() {
        let input = "Finished `dev` profile\ntest result: ok. 0 passed";
        let out = snip(&[s("test")], input);
        assert!(!out.contains("Finished"));
    }

    #[test]
    fn drops_running_lines() {
        let input = "Running unittests src/lib.rs\ntest result: ok. 0 passed";
        let out = snip(&[s("test")], input);
        assert!(!out.contains("Running unittests"));
    }

    #[test]
    fn drops_documenting_line() {
        let input = "Documenting foo v0.1.0\ntest result: ok. 0 passed";
        let out = snip(&[s("test")], input);
        assert!(!out.contains("Documenting"));
    }

    #[test]
    fn drops_locking_line() {
        let input = "Locking 5 packages\ntest result: ok. 0 passed";
        let out = snip(&[s("test")], input);
        assert!(!out.contains("Locking"));
    }

    #[test]
    fn drops_updating_line() {
        let input = "Updating crates.io index\ntest result: ok. 0 passed";
        let out = snip(&[s("test")], input);
        assert!(!out.contains("Updating"));
    }

    #[test]
    fn drops_downloaded_line() {
        let input = "Downloaded foo v1.2.3\ntest result: ok. 0 passed";
        let out = snip(&[s("test")], input);
        assert!(!out.contains("Downloaded"));
    }

    #[test]
    fn drops_per_test_ok() {
        let input = "test foo::bar ... ok\ntest baz::qux ... ok\ntest result: ok. 2 passed";
        let out = snip(&[s("test")], input);
        assert!(!out.contains("foo::bar"));
        assert!(!out.contains("baz::qux"));
        assert!(out.contains("test result"));
    }

    #[test]
    fn drops_per_test_ignored() {
        let input = "test foo::bar ... ignored\ntest result: ok. 0 passed";
        let out = snip(&[s("test")], input);
        assert!(!out.contains("foo::bar"));
    }

    #[test]
    fn keeps_test_result_summary() {
        let input = "test result: ok. 5 passed; 0 failed";
        assert_eq!(snip(&[s("test")], input), input);
    }

    #[test]
    fn keeps_failed_marker() {
        let input = "test foo::bar ... FAILED\ntest result: FAILED";
        let out = snip(&[s("test")], input);
        assert!(out.contains("FAILED"));
        assert!(out.contains("foo::bar"));
    }

    #[test]
    fn keeps_running_n_tests() {
        let input = "running 5 tests\ntest result: ok. 5 passed";
        let out = snip(&[s("test")], input);
        assert!(out.contains("running 5 tests"));
    }

    #[test]
    fn keeps_failures_section() {
        let input = "failures:\n    foo::bar\n    baz::qux";
        let out = snip(&[s("test")], input);
        assert!(out.contains("failures:"));
    }

    #[test]
    fn keeps_failure_block_first_20_lines() {
        let mut input = String::from("---- foo::bar stdout ----\n");
        for i in 0..30 {
            input.push_str(&format!("line {}\n", i));
        }
        let out = snip(&[s("test")], &input);
        for i in 0..20 {
            assert!(out.contains(&format!("line {}", i)), "missing line {}", i);
        }
        assert!(!out.contains("line 25"));
    }

    #[test]
    fn ends_failure_block_on_next_dashes_header() {
        let input = "---- foo::bar stdout ----\nbody\n---- baz::qux stdout ----\n";
        let out = snip(&[s("test")], input);
        assert!(out.contains("---- foo::bar stdout ----"));
        assert!(out.contains("body"));
        assert!(out.contains("---- baz::qux stdout ----"));
    }

    #[test]
    fn keeps_first_five_warnings_drops_rest() {
        let mut input = String::new();
        for i in 0..10 {
            input.push_str(&format!("warning: warn-{}\n", i));
        }
        input.push_str("test result: ok. 0 passed");
        let out = snip(&[s("build")], &input);
        for i in 0..5 {
            assert!(out.contains(&format!("warn-{}", i)));
        }
        for i in 5..10 {
            assert!(!out.contains(&format!("warn-{}", i)));
        }
    }

    #[test]
    fn keeps_warning_with_brackets() {
        let input = "warning[unused_variables]: unused variable: x";
        let out = snip(&[s("build")], input);
        assert!(out.contains("warning[unused_variables]"));
    }

    #[test]
    fn keeps_error_with_brackets() {
        let input = "error[E0308]: mismatched types\n  --> src/lib.rs:5:9\n  = note: expected u32";
        let out = snip(&[s("build")], input);
        assert!(out.contains("error[E0308]"));
        assert!(out.contains("--> src/lib.rs"));
    }

    #[test]
    fn keeps_error_colon() {
        let input = "error: could not compile foo";
        let out = snip(&[s("build")], input);
        assert!(out.contains("error: could not"));
    }

    #[test]
    fn keeps_indented_diagnostic_context() {
        let input = "error[E0308]: mismatched types\n  --> src/main.rs:1:1\n   |\n 1 | fn main() {}";
        let out = snip(&[s("check")], input);
        assert!(out.contains("--> src/main.rs"));
    }

    #[test]
    fn green_run_high_reduction() {
        let mut input = String::from("Compiling foo v0.1.0\nFinished `test` profile\nRunning unittests src/lib.rs\n\nrunning 100 tests\n");
        for i in 0..100 {
            input.push_str(&format!("test mod_a::test_{} ... ok\n", i));
        }
        input.push_str("\ntest result: ok. 100 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out");
        let out = snip(&[s("test")], &input);
        let reduction = 1.0 - (out.len() as f64 / input.len() as f64);
        assert!(reduction > 0.95, "expected >95% reduction, got {}", reduction);
        assert!(out.contains("running 100 tests"));
        assert!(out.contains("test result: ok. 100 passed"));
    }

    #[test]
    fn mixed_failure_run_85_percent_reduction() {
        let mut input = String::from(
            "Compiling foo v0.1.0\nFinished `test` profile\nRunning unittests src/lib.rs\n\nrunning 200 tests\n",
        );
        for i in 0..199 {
            input.push_str(&format!("test mod_a::test_{} ... ok\n", i));
        }
        input.push_str("test mod_a::test_199 ... FAILED\n\nfailures:\n\n---- mod_a::test_199 stdout ----\n");
        for i in 0..30 {
            input.push_str(&format!("backtrace line {}\n", i));
        }
        input.push_str("\nfailures:\n    mod_a::test_199\n\ntest result: FAILED. 199 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out");
        let out = snip(&[s("test")], &input);
        let reduction = 1.0 - (out.len() as f64 / input.len() as f64);
        assert!(reduction > 0.85, "expected >85% reduction on mixed failure, got {}", reduction);
        assert!(out.contains("running 200 tests"));
        assert!(out.contains("FAILED"));
        assert!(out.contains("mod_a::test_199"));
        assert!(out.contains("backtrace line 0"));
    }

    #[test]
    fn all_fail_preserves_diagnostics() {
        let mut input = String::from("running 3 tests\n");
        for i in 0..3 {
            input.push_str(&format!("test t_{} ... FAILED\n", i));
        }
        input.push('\n');
        for i in 0..3 {
            input.push_str(&format!("---- t_{} stdout ----\n", i));
            input.push_str("thread 'main' panicked at src/lib.rs:5:5:\nassertion failed: false\nnote: run with `RUST_BACKTRACE=1`\n\n");
        }
        input.push_str("\nfailures:\n    t_0\n    t_1\n    t_2\n\ntest result: FAILED. 0 passed; 3 failed");
        let out = snip(&[s("test")], &input);
        for i in 0..3 {
            assert!(out.contains(&format!("t_{}", i)), "lost name t_{}", i);
        }
        assert!(out.contains("assertion failed: false"));
        assert!(out.contains("test result: FAILED"));
    }

    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(snip(&[s("test")], ""), "");
    }

    #[test]
    fn keeps_trailing_newline_if_present() {
        let input = "test result: ok. 0 passed\n";
        assert_eq!(snip(&[s("test")], input), input);
    }

    #[test]
    fn no_trailing_newline_preserved() {
        let input = "test result: ok. 0 passed";
        assert!(!snip(&[s("test")], input).ends_with('\n'));
    }

    #[test]
    fn fixture_green_byte_reduction_at_least_95_percent() {
        let input = include_str!("../../../tests/fixtures/cargo-test-green.txt");
        let out = snip(&[s("test")], input);
        let reduction = 1.0 - (out.len() as f64 / input.len() as f64);
        assert!(
            reduction >= 0.95,
            "expected ≥95% reduction on green fixture, got {:.2}%",
            reduction * 100.0
        );
        assert!(out.contains("test result: ok"));
        assert!(out.contains("running 50 tests"));
    }

    #[test]
    fn fixture_mixed_byte_reduction_at_least_85_percent() {
        let input = include_str!("../../../tests/fixtures/cargo-test-mixed.txt");
        let out = snip(&[s("test")], input);
        let reduction = 1.0 - (out.len() as f64 / input.len() as f64);
        assert!(
            reduction >= 0.85,
            "expected ≥85% reduction on mixed-failure fixture, got {:.2}%",
            reduction * 100.0
        );
        assert!(out.contains("FAILED"));
        assert!(out.contains("cmd::b::failing_test"));
        assert!(out.contains("assertion `left == right` failed"));
    }

    #[test]
    fn fixture_byte_measurements() {
        let green = include_str!("../../../tests/fixtures/cargo-test-green.txt");
        let mixed = include_str!("../../../tests/fixtures/cargo-test-mixed.txt");
        let go = snip(&[s("test")], green);
        let mo = snip(&[s("test")], mixed);
        eprintln!("[snip-bench] cargo green: {} → {} bytes  ({:.1}% reduction)",
            green.len(), go.len(), (1.0 - go.len() as f64 / green.len() as f64) * 100.0);
        eprintln!("[snip-bench] cargo mixed: {} → {} bytes  ({:.1}% reduction)",
            mixed.len(), mo.len(), (1.0 - mo.len() as f64 / mixed.len() as f64) * 100.0);
    }

    #[test]
    fn fixture_green_keeps_warning() {
        let input = include_str!("../../../tests/fixtures/cargo-test-mixed.txt");
        let out = snip(&[s("test")], input);
        assert!(out.contains("warning:"));
    }
}
