//! Node test/build truncator. Covers `npm test`, `pnpm test`, `yarn test`,
//! `jest`, `vitest`, `npm run build`, `pnpm build`, etc.
//!
//! Test mode (default for `*test*`, `jest`, `vitest`):
//!   Keep: `^FAIL`, `^●` (failure-block bullet), `Expected:` / `Received:`
//!         lines, error stacks, summary block (`Tests:`, `Test Files:`).
//!   Drop: `^PASS`, `✓` checkmarks, ASCII spinner frames, watch banners.
//!
//! Build mode (any `build` subcommand):
//!   Keep: `error TS\d+:`, `Failed to compile`, `^✗`, error blocks.
//!   Drop: progress bars, `Compiling …`, watch-mode reload notices.

use regex::Regex;
use std::sync::OnceLock;

pub fn snip(head: &str, args: &[String], input: &str) -> String {
    if head == "jest" || head == "vitest" {
        return snip_test(input);
    }
    let sub = match first_non_flag(args) {
        Some(s) => s,
        None => return input.to_string(),
    };
    if sub == "test" || sub == "t" {
        return snip_test(input);
    }
    if sub == "build" || sub == "b" || sub == "compile" {
        return snip_build(input);
    }
    if sub == "run" {
        if let Some(script) = args.iter().find(|a| !a.starts_with('-') && a.as_str() != "run") {
            if script == "test" || script.contains("test") {
                return snip_test(input);
            }
            if script == "build" || script.contains("build") {
                return snip_build(input);
            }
        }
    }
    input.to_string()
}

fn first_non_flag(args: &[String]) -> Option<&str> {
    for a in args {
        if !a.starts_with('-') {
            return Some(a.as_str());
        }
    }
    None
}

static SPINNER_RE: OnceLock<Regex> = OnceLock::new();
fn spinner_re() -> &'static Regex {
    SPINNER_RE.get_or_init(|| {
        Regex::new(r"^[\s]*[⠁⠂⠄⡀⢀⠠⠐⠈|/\-\\]+\s").unwrap()
    })
}

static PROGRESS_RE: OnceLock<Regex> = OnceLock::new();
fn progress_re() -> &'static Regex {
    PROGRESS_RE.get_or_init(|| Regex::new(r"^\s*\d+%\s*\[").unwrap())
}

pub(crate) fn snip_test(input: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in input.lines() {
        if is_pass_line(line) {
            continue;
        }
        if is_check_line(line) {
            continue;
        }
        if is_spinner_line(line) {
            continue;
        }
        if is_watch_banner(line) {
            continue;
        }
        out.push(line);
    }
    join_with_newline(&out, input)
}

pub(crate) fn snip_build(input: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in input.lines() {
        if is_progress_bar(line) {
            continue;
        }
        if is_spinner_line(line) {
            continue;
        }
        if is_build_noise(line) {
            continue;
        }
        if is_watch_banner(line) {
            continue;
        }
        out.push(line);
    }
    join_with_newline(&out, input)
}

fn is_pass_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("PASS ") || t == "PASS"
}

fn is_check_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("\u{2713}") || t.starts_with("✓ ")
}

fn is_spinner_line(line: &str) -> bool {
    spinner_re().is_match(line)
}

fn is_progress_bar(line: &str) -> bool {
    progress_re().is_match(line)
        || line.contains("\u{2588}")
        || line.contains("\u{2591}")
}

fn is_build_noise(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("Compiling ")
        || t.starts_with("Building ")
        || t.starts_with("Bundling ")
        || t.starts_with("Transforming ")
        || t.starts_with("[webpack]")
        || t == "Done."
        || t.starts_with("> ")
}

fn is_watch_banner(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("watching for file changes")
        || t.starts_with("Waiting for file changes")
        || t.starts_with("Watching for file changes")
        || t.starts_with("Watch Usage")
        || t.starts_with("Watch usage")
        || t.starts_with("Press ")
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
    fn jest_drops_pass_line() {
        let out = snip("jest", &[], "PASS src/foo.test.ts");
        assert_eq!(out, "");
    }

    #[test]
    fn jest_keeps_fail_line() {
        let out = snip("jest", &[], "FAIL src/foo.test.ts");
        assert_eq!(out, "FAIL src/foo.test.ts");
    }

    #[test]
    fn jest_drops_check_marker() {
        let out = snip("jest", &[], "  ✓ adds two numbers (1ms)");
        assert_eq!(out, "");
    }

    #[test]
    fn jest_keeps_bullet_failure() {
        let out = snip("jest", &[], "● failing test");
        assert!(out.contains("● failing test"));
    }

    #[test]
    fn jest_keeps_expected_received() {
        let input = "Expected: 1\nReceived: 2";
        let out = snip("jest", &[], input);
        assert_eq!(out, input);
    }

    #[test]
    fn vitest_treated_as_test() {
        let out = snip("vitest", &[], "PASS  src/foo.test.ts");
        assert_eq!(out, "");
    }

    #[test]
    fn npm_test_dispatches_test_truncator() {
        let out = snip("npm", &[s("test")], "PASS  src/foo.test.ts");
        assert_eq!(out, "");
    }

    #[test]
    fn pnpm_test_dispatches_test_truncator() {
        let out = snip("pnpm", &[s("test")], "PASS  src/foo.test.ts");
        assert_eq!(out, "");
    }

    #[test]
    fn yarn_test_dispatches_test_truncator() {
        let out = snip("yarn", &[s("test")], "PASS  src/foo.test.ts");
        assert_eq!(out, "");
    }

    #[test]
    fn npm_run_test_dispatches_test_truncator() {
        let out = snip("npm", &[s("run"), s("test")], "PASS x");
        assert_eq!(out, "");
    }

    #[test]
    fn npm_run_test_unit_dispatches_test_truncator() {
        let out = snip("npm", &[s("run"), s("test:unit")], "PASS x");
        assert_eq!(out, "");
    }

    #[test]
    fn npm_run_build_dispatches_build_truncator() {
        let input = "Compiling foo.ts\n50% [#####             ] building";
        let out = snip("npm", &[s("run"), s("build")], input);
        assert_eq!(out, "");
    }

    #[test]
    fn npm_build_keeps_ts_error() {
        let input = "src/foo.ts:1:1 - error TS2304: Cannot find name 'foo'.";
        let out = snip("npm", &[s("build")], input);
        assert!(out.contains("error TS2304"));
    }

    #[test]
    fn npm_build_keeps_failed_to_compile() {
        let input = "Failed to compile\nsrc/foo.ts:1:1 - error TS2304";
        let out = snip("npm", &[s("build")], input);
        assert!(out.contains("Failed to compile"));
    }

    #[test]
    fn build_drops_progress_bar() {
        let input = "  50% [############        ] building modules";
        let out = snip("npm", &[s("build")], input);
        assert_eq!(out, "");
    }

    #[test]
    fn build_drops_compiling_line() {
        let input = "Compiling src/foo.ts";
        let out = snip("npm", &[s("build")], input);
        assert_eq!(out, "");
    }

    #[test]
    fn build_drops_watch_banner() {
        let input = "watching for file changes...";
        let out = snip("npm", &[s("build")], input);
        assert_eq!(out, "");
    }

    #[test]
    fn build_keeps_x_marker() {
        let input = "✗ build failed";
        let out = snip("npm", &[s("build")], input);
        assert!(out.contains("✗ build failed"));
    }

    #[test]
    fn jest_drops_spinner_chars() {
        let input = "⠁ Running tests...";
        let out = snip("jest", &[], input);
        assert_eq!(out, "");
    }

    #[test]
    fn jest_drops_watch_help() {
        let input = "Watch Usage:\nPress a to run all tests";
        let out = snip("jest", &[], input);
        assert_eq!(out, "");
    }

    #[test]
    fn jest_keeps_summary() {
        let input = "Tests:       1 failed, 49 passed, 50 total";
        let out = snip("jest", &[], input);
        assert!(out.contains("1 failed, 49 passed"));
    }

    #[test]
    fn green_test_run_high_reduction() {
        let mut input = String::from("PASS  src/foo.test.ts\n");
        for i in 0..100 {
            input.push_str(&format!("  ✓ test_{} (1ms)\n", i));
        }
        input.push_str("Tests:       100 passed, 100 total");
        let out = snip("jest", &[], &input);
        let reduction = 1.0 - (out.len() as f64 / input.len() as f64);
        assert!(reduction > 0.85, "expected >85% reduction, got {}", reduction);
        assert!(out.contains("Tests:"));
    }

    #[test]
    fn all_fail_preserves_diagnostics() {
        let input = "FAIL src/foo.test.ts\n● test_foo\n  Expected: 1\n  Received: 2\nTests: 1 failed, 0 passed";
        let out = snip("jest", &[], input);
        assert!(out.contains("FAIL"));
        assert!(out.contains("Expected: 1"));
        assert!(out.contains("Received: 2"));
        assert!(out.contains("Tests: 1 failed"));
    }

    #[test]
    fn passthrough_for_unknown_subcommand() {
        let input = "anything goes here";
        let out = snip("npm", &[s("ls")], input);
        assert_eq!(out, input);
    }

    #[test]
    fn passthrough_for_npm_no_args() {
        let input = "anything";
        assert_eq!(snip("npm", &[], input), input);
    }

    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(snip("jest", &[], ""), "");
    }
}
