//! Git output truncators. Two subcommands:
//!
//! `git log`  — first 30 commit lines + ellipsis + last 5 commit lines.
//! Deterministic 35-line cap regardless of input size.
//!
//! `git diff` — preserve `diff --git`, `@@ … @@` hunk headers, file headers
//! (`+++`/`---`/`index`), and ±10 lines around each hunk header. Hunk
//! middles longer than 50 lines collapse to head/tail with an ellipsis.
//!
//! Other subcommands → identity passthrough.

pub fn snip(args: &[String], input: &str) -> String {
    let sub = match first_non_flag(args) {
        Some(s) => s,
        None => return input.to_string(),
    };
    match sub {
        "log" => snip_log(input),
        "diff" => snip_diff(input),
        "show" => snip_diff(input),
        _ => input.to_string(),
    }
}

fn first_non_flag(args: &[String]) -> Option<&str> {
    for a in args {
        if !a.starts_with('-') {
            return Some(a.as_str());
        }
    }
    None
}

const LOG_HEAD: usize = 29;
const LOG_TAIL: usize = 5;
const HUNK_CONTEXT: usize = 10;
const HUNK_BODY_MAX: usize = 50;
const HUNK_BODY_KEEP: usize = 20;

pub(crate) fn snip_log(input: &str) -> String {
    let lines: Vec<&str> = input.lines().collect();
    let trailing_nl = input.ends_with('\n');
    let total = lines.len();
    if total <= LOG_HEAD + LOG_TAIL + 1 {
        return input.to_string();
    }
    let mut out: Vec<String> = Vec::with_capacity(LOG_HEAD + LOG_TAIL + 1);
    for line in lines.iter().take(LOG_HEAD) {
        out.push((*line).to_string());
    }
    out.push(format!("... ({} commits omitted) ...", total - LOG_HEAD - LOG_TAIL));
    for line in lines.iter().skip(total - LOG_TAIL) {
        out.push((*line).to_string());
    }
    let mut s = out.join("\n");
    if trailing_nl {
        s.push('\n');
    }
    s
}

pub(crate) fn snip_diff(input: &str) -> String {
    let lines: Vec<&str> = input.lines().collect();
    let trailing_nl = input.ends_with('\n');
    let mut out: Vec<String> = Vec::with_capacity(lines.len());

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if is_hunk_header(line) {
            out.push(line.to_string());
            let mut j = i + 1;
            let mut body: Vec<&str> = Vec::new();
            while j < lines.len() && !is_hunk_header(lines[j]) && !is_diff_header(lines[j]) {
                body.push(lines[j]);
                j += 1;
            }
            push_hunk_body(&mut out, &body);
            i = j;
            continue;
        }
        if is_diff_header(line) {
            out.push(line.to_string());
            i += 1;
            continue;
        }
        out.push(line.to_string());
        i += 1;
    }

    let mut s = out.join("\n");
    if trailing_nl {
        s.push('\n');
    }
    s
}

fn is_hunk_header(line: &str) -> bool {
    line.starts_with("@@ ") && line.contains(" @@")
}

fn is_diff_header(line: &str) -> bool {
    line.starts_with("diff --git ")
        || line.starts_with("index ")
        || line.starts_with("--- ")
        || line.starts_with("+++ ")
        || line.starts_with("new file mode ")
        || line.starts_with("deleted file mode ")
        || line.starts_with("rename from ")
        || line.starts_with("rename to ")
        || line.starts_with("similarity index ")
        || line.starts_with("Binary files ")
}

fn push_hunk_body(out: &mut Vec<String>, body: &[&str]) {
    let len = body.len();
    if len == 0 {
        return;
    }
    if len <= HUNK_CONTEXT * 2 || len <= HUNK_BODY_MAX {
        for line in body {
            out.push((*line).to_string());
        }
        return;
    }
    for line in body.iter().take(HUNK_BODY_KEEP) {
        out.push((*line).to_string());
    }
    out.push(format!("... ({} lines collapsed) ...", len - HUNK_BODY_KEEP - HUNK_CONTEXT));
    for line in body.iter().skip(len - HUNK_CONTEXT) {
        out.push((*line).to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> String {
        v.to_string()
    }

    #[test]
    fn log_passthrough_when_short() {
        let input = (0..10).map(|i| format!("commit {}", i)).collect::<Vec<_>>().join("\n");
        assert_eq!(snip(&[s("log")], &input), input);
    }

    #[test]
    fn log_truncates_long_input_to_35_lines_total() {
        let input = (0..200).map(|i| format!("commit {}", i)).collect::<Vec<_>>().join("\n");
        let out = snip(&[s("log")], &input);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), LOG_HEAD + LOG_TAIL + 1);
        assert_eq!(lines.len(), 35);
        assert!(lines[LOG_HEAD].contains("commits omitted"));
        assert_eq!(lines[0], "commit 0");
        assert_eq!(lines[LOG_HEAD - 1], "commit 28");
        assert_eq!(lines[LOG_HEAD + 1], "commit 195");
        assert_eq!(lines[LOG_HEAD + LOG_TAIL], "commit 199");
    }

    #[test]
    fn log_deterministic_for_200() {
        let input = (0..200).map(|i| format!("commit {}", i)).collect::<Vec<_>>().join("\n");
        let a = snip(&[s("log")], &input);
        let b = snip(&[s("log")], &input);
        assert_eq!(a, b);
        assert_eq!(a.lines().count(), 35);
    }

    #[test]
    fn log_keeps_first_head_lines() {
        let input = (0..100).map(|i| format!("c{}", i)).collect::<Vec<_>>().join("\n");
        let out = snip(&[s("log")], &input);
        for i in 0..LOG_HEAD {
            assert!(
                out.lines().any(|l| l == format!("c{}", i)),
                "missing line c{}",
                i
            );
        }
    }

    #[test]
    fn log_keeps_last_5_lines() {
        let input = (0..100).map(|i| format!("c{}", i)).collect::<Vec<_>>().join("\n");
        let out = snip(&[s("log")], &input);
        for i in 95..100 {
            assert!(out.lines().any(|l| l == format!("c{}", i)));
        }
    }

    #[test]
    fn log_preserves_trailing_newline() {
        let input = (0..200).map(|i| format!("commit {}", i)).collect::<Vec<_>>().join("\n") + "\n";
        let out = snip(&[s("log")], &input);
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn log_no_trailing_newline() {
        let input = (0..200).map(|i| format!("commit {}", i)).collect::<Vec<_>>().join("\n");
        let out = snip(&[s("log")], &input);
        assert!(!out.ends_with('\n'));
    }

    #[test]
    fn diff_keeps_diff_header() {
        let input = "diff --git a/foo b/foo\nindex abc..def 100644\n--- a/foo\n+++ b/foo\n@@ -1,1 +1,1 @@\n-old\n+new";
        let out = snip(&[s("diff")], input);
        assert!(out.contains("diff --git a/foo b/foo"));
        assert!(out.contains("index abc..def"));
        assert!(out.contains("--- a/foo"));
        assert!(out.contains("+++ b/foo"));
        assert!(out.contains("@@ -1,1 +1,1 @@"));
    }

    #[test]
    fn diff_passthrough_small_hunk() {
        let input = "diff --git a/x b/x\n@@ -1,3 +1,3 @@\n line1\n-line2\n+line2'\n line3";
        assert_eq!(snip(&[s("diff")], input), input);
    }

    #[test]
    fn diff_collapses_huge_hunk_body() {
        let mut input = String::from("diff --git a/x b/x\n@@ -1,200 +1,200 @@\n");
        for i in 0..200 {
            input.push_str(&format!(" ctx{}\n", i));
        }
        let out = snip(&[s("diff")], &input);
        assert!(out.contains("@@ -1,200 +1,200 @@"));
        assert!(out.contains("lines collapsed"));
        assert!(out.contains(" ctx0"));
        assert!(out.contains(" ctx199"));
        assert!(out.lines().count() < 200);
    }

    #[test]
    fn diff_keeps_each_hunk_independently() {
        let mut input = String::from("diff --git a/x b/x\n@@ -1,1 +1,1 @@\n-a\n+b\n@@ -10,1 +10,1 @@\n-c\n+d\n");
        input.push_str("@@ -20,1 +20,1 @@\n-e\n+f\n");
        let out = snip(&[s("diff")], &input);
        assert_eq!(out.matches("@@").count(), 6); // 3 hunks * 2 @@ markers each
    }

    #[test]
    fn diff_keeps_binary_marker() {
        let input = "Binary files a/foo and b/foo differ";
        assert_eq!(snip(&[s("diff")], input), input);
    }

    #[test]
    fn diff_keeps_rename_headers() {
        let input = "diff --git a/old b/new\nsimilarity index 100%\nrename from old\nrename to new";
        let out = snip(&[s("diff")], input);
        assert!(out.contains("similarity index 100%"));
        assert!(out.contains("rename from old"));
        assert!(out.contains("rename to new"));
    }

    #[test]
    fn diff_show_uses_same_truncator() {
        let mut input = String::from("commit abc\n\ndiff --git a/x b/x\n@@ -1,200 +1,200 @@\n");
        for i in 0..200 {
            input.push_str(&format!(" ctx{}\n", i));
        }
        let out = snip(&[s("show")], &input);
        assert!(out.contains("@@ -1,200 +1,200 @@"));
        assert!(out.contains("collapsed"));
    }

    #[test]
    fn passthrough_for_other_subcommands() {
        let input = "On branch main\nnothing to commit";
        assert_eq!(snip(&[s("status")], input), input);
    }

    #[test]
    fn passthrough_when_no_subcommand() {
        let input = "anything";
        assert_eq!(snip(&[], input), input);
    }

    #[test]
    fn skips_leading_flags_to_find_subcommand() {
        let input = (0..200).map(|i| format!("c{}", i)).collect::<Vec<_>>().join("\n");
        let out = snip(&[s("--no-pager"), s("log")], &input);
        assert!(out.lines().count() <= LOG_HEAD + LOG_TAIL + 1);
    }

    #[test]
    fn diff_empty_returns_empty() {
        assert_eq!(snip(&[s("diff")], ""), "");
    }

    #[test]
    fn log_empty_returns_empty() {
        assert_eq!(snip(&[s("log")], ""), "");
    }

    #[test]
    fn log_log_oneline_200_is_35_lines() {
        let input = (0..200).map(|i| format!("abc{:04} commit msg {}", i, i)).collect::<Vec<_>>().join("\n");
        let out = snip(&[s("log"), s("--oneline"), s("-200")], &input);
        assert_eq!(out.lines().count(), 35);
    }

    #[test]
    fn diff_keeps_no_newline_marker() {
        let input = "diff --git a/x b/x\n@@ -1,1 +1,1 @@\n-old\n+new\n\\ No newline at end of file";
        let out = snip(&[s("diff")], input);
        assert!(out.contains("\\ No newline at end of file"));
    }

    #[test]
    fn diff_multiple_files() {
        let mut input = String::from("diff --git a/x b/x\n@@ -1,1 +1,1 @@\n-a\n+b\n");
        input.push_str("diff --git a/y b/y\n@@ -1,1 +1,1 @@\n-c\n+d\n");
        let out = snip(&[s("diff")], &input);
        assert!(out.contains("a/x"));
        assert!(out.contains("a/y"));
        assert_eq!(out.matches("diff --git").count(), 2);
    }

    #[test]
    fn diff_long_body_with_50_lines_no_collapse() {
        let mut input = String::from("diff --git a/x b/x\n@@ -1,50 +1,50 @@\n");
        for i in 0..50 {
            input.push_str(&format!(" ctx{}\n", i));
        }
        let out = snip(&[s("diff")], &input);
        assert!(!out.contains("collapsed"));
        for i in 0..50 {
            assert!(out.contains(&format!(" ctx{}", i)));
        }
    }

    #[test]
    fn diff_body_with_51_lines_collapses() {
        let mut input = String::from("diff --git a/x b/x\n@@ -1,51 +1,51 @@\n");
        for i in 0..51 {
            input.push_str(&format!(" ctx{}\n", i));
        }
        let out = snip(&[s("diff")], &input);
        assert!(out.contains("collapsed"));
    }

    #[test]
    fn log_with_short_input_passthrough() {
        let input = (0..29).map(|i| format!("c{}", i)).collect::<Vec<_>>().join("\n");
        assert_eq!(snip(&[s("log")], &input), input);
    }

    #[test]
    fn fixture_log_200_collapses_to_35_lines_deterministic() {
        let input = include_str!("../../../tests/fixtures/git-log-200.txt");
        let a = snip(&[s("log")], input);
        let b = snip(&[s("log")], input);
        assert_eq!(a, b, "output must be deterministic across runs");
        assert_eq!(a.lines().count(), 35);
        assert!(a.contains("abc0001"));
        assert!(a.contains("abc0200"));
        assert!(a.contains("commits omitted"));
    }

    #[test]
    fn log_preserves_long_log_header() {
        let mut input = String::new();
        for i in 0..50 {
            input.push_str(&format!("commit hash{:04}\nAuthor: foo\nDate: today\n\n    msg {}\n\n", i, i));
        }
        let lines = input.lines().count();
        assert!(lines > 35);
        let out = snip(&[s("log")], &input);
        assert!(out.lines().count() <= LOG_HEAD + LOG_TAIL + 1);
    }
}
