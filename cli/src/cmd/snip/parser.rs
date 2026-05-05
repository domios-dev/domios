//! Quote-aware shell parsing primitives for `codeterm snip` and (later) the
//! Bash hook wrapper. Pure Rust — no shell-out, no platform syscalls.
//!
//! Architecture mirrors `references/opencode-snip/src/index.ts` (env-var
//! prefix preservation, shell-operator splitting, quote-aware pipe
//! detection, unproxy-able-builtins list); the implementation is original.

/// Shell builtins / control-flow words that must never be wrapped — running
/// them in a child process either has no effect (`cd`, `export`) or actively
/// breaks the parent shell's state (`source`, `eval`, `exec`).
pub const SHELL_BUILTINS: &[&str] = &[
    ".",
    "alias",
    "break",
    "cd",
    "command",
    "continue",
    "declare",
    "eval",
    "exec",
    "exit",
    "export",
    "function",
    "let",
    "local",
    "popd",
    "pushd",
    "readonly",
    "return",
    "set",
    "shift",
    "shopt",
    "source",
    "trap",
    "typeset",
    "ulimit",
    "umask",
    "unalias",
    "unset",
];

/// Split a single argv token into `(env_prefix, rest)`. The env prefix is
/// the longest leading run of `KEY=val` assignments, including the run of
/// whitespace that separates each from the next token (so concatenating
/// `env_prefix + rest` reconstructs the input verbatim).
pub fn split_env_prefix(s: &str) -> (String, String) {
    let mut prefix = String::new();
    let mut rest = s;
    loop {
        match consume_env_assignment(rest) {
            Some((env, after)) => {
                prefix.push_str(env);
                rest = after;
            }
            None => return (prefix, rest.to_string()),
        }
    }
}

/// If `s` starts with `IDENT=value`, return `(env_token, remainder)` where
/// `env_token` includes both the assignment and any run of trailing
/// whitespace. Returns `None` if the prefix does not match.
fn consume_env_assignment(s: &str) -> Option<(&str, &str)> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    if !is_ident_start(bytes[0]) {
        return None;
    }
    let mut i = 1;
    while i < bytes.len() && is_ident_continue(bytes[i]) {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'=' {
        return None;
    }
    i += 1;
    while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    Some((&s[..i], &s[i..]))
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Look up `head` in the builtin list. Case-sensitive — Unix shells are.
pub fn is_shell_builtin(head: &str) -> bool {
    SHELL_BUILTINS.iter().any(|b| *b == head)
}

/// Find the index of the first unquoted single-pipe character in `s`.
/// Returns `None` if every `|` is inside quotes or part of `||`.
pub fn find_first_pipe(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'\\' if !in_single => {
                i += 2;
                continue;
            }
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'|' if !in_single && !in_double => {
                let next = bytes.get(i + 1).copied();
                let prev = if i > 0 { Some(bytes[i - 1]) } else { None };
                if next == Some(b'|') || prev == Some(b'|') {
                    i += 1;
                    continue;
                }
                return Some(i);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// One segment in a shell pipeline split — either a sub-command or the
/// raw operator that joined it to the previous one.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Segment {
    Command(String),
    Operator(String),
}

/// Split a shell command string on top-level operators (`&&`, `||`, `;`,
/// trailing `&`). Quotes and escapes suppress the split. Pipes (`|`) are
/// **not** split here — they are handled separately by `find_first_pipe`,
/// matching opencode-snip's behaviour of only wrapping the head of a pipe.
pub fn split_on_operators(s: &str) -> Vec<Segment> {
    let bytes = s.as_bytes();
    let mut out: Vec<Segment> = Vec::new();
    let mut cursor = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'\\' if !in_single => {
                i += 2;
                continue;
            }
            b'\'' if !in_double => {
                in_single = !in_single;
                i += 1;
                continue;
            }
            b'"' if !in_single => {
                in_double = !in_double;
                i += 1;
                continue;
            }
            _ => {}
        }
        if in_single || in_double {
            i += 1;
            continue;
        }
        let two = bytes.get(i..i + 2);
        if two == Some(b"&&") || two == Some(b"||") {
            push_command(&mut out, &s[cursor..i]);
            out.push(Segment::Operator(s[i..i + 2].to_string()));
            i += 2;
            cursor = i;
            continue;
        }
        if c == b';' {
            push_command(&mut out, &s[cursor..i]);
            out.push(Segment::Operator(";".to_string()));
            i += 1;
            cursor = i;
            continue;
        }
        if c == b'&' {
            let next = bytes.get(i + 1).copied();
            let prev = if i > 0 { Some(bytes[i - 1]) } else { None };
            let preceded_by_ws = matches!(prev, Some(b' ') | Some(b'\t'));
            let followed_by_redirect = matches!(next, Some(b'>'));
            if next != Some(b'&') && preceded_by_ws && !followed_by_redirect {
                push_command(&mut out, &s[cursor..i]);
                out.push(Segment::Operator("&".to_string()));
                i += 1;
                cursor = i;
                continue;
            }
        }
        i += 1;
    }
    if cursor < bytes.len() {
        push_command(&mut out, &s[cursor..]);
    }
    out
}

fn push_command(out: &mut Vec<Segment>, s: &str) {
    if !s.is_empty() {
        out.push(Segment::Command(s.to_string()));
    }
}

/// Split a single command segment into `(env_prefix, head_cmd, args)`.
/// Tokenises with quote awareness: `'foo bar'`, `"baz qux"`, and
/// `\<space>` all stay together. Returns `None` if the segment is empty
/// after env-prefix removal.
pub fn parse_segment(seg: &str) -> Option<(String, String, Vec<String>)> {
    let (env_prefix, rest) = split_env_prefix(seg);
    let tokens = tokenize(&rest);
    let mut iter = tokens.into_iter();
    let head = iter.next()?;
    if head.is_empty() {
        return None;
    }
    let args: Vec<String> = iter.collect();
    Some((env_prefix, head, args))
}

/// Quote-aware whitespace tokeniser. Removes quoting characters from the
/// emitted tokens; `\<char>` outside single-quotes is interpreted as a
/// literal `<char>`. Good enough for command-head identification, not a
/// full POSIX shell parser.
pub fn tokenize(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut have_token = false;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'\\' && !in_single {
            if let Some(&next) = bytes.get(i + 1) {
                cur.push(next as char);
                have_token = true;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if c == b'\'' && !in_double {
            in_single = !in_single;
            have_token = true;
            i += 1;
            continue;
        }
        if c == b'"' && !in_single {
            in_double = !in_double;
            have_token = true;
            i += 1;
            continue;
        }
        if c.is_ascii_whitespace() && !in_single && !in_double {
            if have_token {
                out.push(std::mem::take(&mut cur));
                have_token = false;
            }
            i += 1;
            continue;
        }
        cur.push(c as char);
        have_token = true;
        i += 1;
    }
    if have_token {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_prefix_none_when_no_assignment() {
        let (p, r) = split_env_prefix("cargo test");
        assert_eq!(p, "");
        assert_eq!(r, "cargo test");
    }

    #[test]
    fn env_prefix_single_assignment() {
        let (p, r) = split_env_prefix("FOO=bar baz");
        assert_eq!(p, "FOO=bar ");
        assert_eq!(r, "baz");
    }

    #[test]
    fn env_prefix_multiple_assignments() {
        let (p, r) = split_env_prefix("RUST_LOG=debug FOO=bar cargo test");
        assert_eq!(p, "RUST_LOG=debug FOO=bar ");
        assert_eq!(r, "cargo test");
    }

    #[test]
    fn env_prefix_underscore_ident() {
        let (p, r) = split_env_prefix("_X=1 cmd");
        assert_eq!(p, "_X=1 ");
        assert_eq!(r, "cmd");
    }

    #[test]
    fn env_prefix_digits_in_ident_after_first() {
        let (p, r) = split_env_prefix("PATH2=/foo cmd");
        assert_eq!(p, "PATH2=/foo ");
        assert_eq!(r, "cmd");
    }

    #[test]
    fn env_prefix_empty_value() {
        let (p, r) = split_env_prefix("FOO= cmd");
        assert_eq!(p, "FOO= ");
        assert_eq!(r, "cmd");
    }

    #[test]
    fn env_prefix_empty_value_no_space() {
        let (p, r) = split_env_prefix("FOO=");
        assert_eq!(p, "FOO=");
        assert_eq!(r, "");
    }

    #[test]
    fn env_prefix_rejects_leading_digit() {
        let (p, r) = split_env_prefix("1FOO=bar cmd");
        assert_eq!(p, "");
        assert_eq!(r, "1FOO=bar cmd");
    }

    #[test]
    fn env_prefix_rejects_dash_in_ident() {
        let (p, r) = split_env_prefix("FOO-BAR=x cmd");
        assert_eq!(p, "");
        assert_eq!(r, "FOO-BAR=x cmd");
    }

    #[test]
    fn env_prefix_rejects_no_equals() {
        let (p, r) = split_env_prefix("FOO cmd");
        assert_eq!(p, "");
        assert_eq!(r, "FOO cmd");
    }

    #[test]
    fn env_prefix_value_with_slash() {
        let (p, r) = split_env_prefix("PATH=/usr/bin cmd");
        assert_eq!(p, "PATH=/usr/bin ");
        assert_eq!(r, "cmd");
    }

    #[test]
    fn env_prefix_multiple_spaces_preserved() {
        let (p, r) = split_env_prefix("FOO=bar  cmd");
        assert_eq!(p, "FOO=bar  ");
        assert_eq!(r, "cmd");
    }

    #[test]
    fn env_prefix_only() {
        let (p, r) = split_env_prefix("FOO=bar BAZ=qux");
        assert_eq!(p, "FOO=bar BAZ=qux");
        assert_eq!(r, "");
    }

    #[test]
    fn shell_builtin_cd() {
        assert!(is_shell_builtin("cd"));
    }

    #[test]
    fn shell_builtin_source() {
        assert!(is_shell_builtin("source"));
    }

    #[test]
    fn shell_builtin_dot() {
        assert!(is_shell_builtin("."));
    }

    #[test]
    fn shell_builtin_export() {
        assert!(is_shell_builtin("export"));
    }

    #[test]
    fn shell_builtin_eval() {
        assert!(is_shell_builtin("eval"));
    }

    #[test]
    fn shell_builtin_exec() {
        assert!(is_shell_builtin("exec"));
    }

    #[test]
    fn shell_builtin_set() {
        assert!(is_shell_builtin("set"));
    }

    #[test]
    fn shell_builtin_unset() {
        assert!(is_shell_builtin("unset"));
    }

    #[test]
    fn shell_builtin_rejects_cargo() {
        assert!(!is_shell_builtin("cargo"));
    }

    #[test]
    fn shell_builtin_rejects_capitalised_cd() {
        assert!(!is_shell_builtin("CD"));
    }

    #[test]
    fn shell_builtin_rejects_path() {
        assert!(!is_shell_builtin("/usr/bin/cd"));
    }

    #[test]
    fn pipe_simple() {
        assert_eq!(find_first_pipe("a | b"), Some(2));
    }

    #[test]
    fn pipe_inside_single_quotes_ignored() {
        assert_eq!(find_first_pipe("a 'x|y' b"), None);
    }

    #[test]
    fn pipe_inside_double_quotes_ignored() {
        assert_eq!(find_first_pipe("a \"x|y\" b"), None);
    }

    #[test]
    fn pipe_logical_or_ignored() {
        assert_eq!(find_first_pipe("a || b"), None);
    }

    #[test]
    fn pipe_after_logical_or_returns_real_pipe() {
        let s = "a || b | c";
        let idx = find_first_pipe(s).expect("pipe present");
        assert_eq!(&s[idx..idx + 1], "|");
        assert!(idx > 5);
    }

    #[test]
    fn pipe_double_quote_inside_single_is_literal() {
        assert_eq!(find_first_pipe("'a\"b' | c"), Some(6));
    }

    #[test]
    fn pipe_escaped_pipe_skipped() {
        assert_eq!(find_first_pipe(r"a \| b | c"), Some(7));
    }

    #[test]
    fn pipe_no_pipe() {
        assert_eq!(find_first_pipe("cargo test"), None);
    }

    #[test]
    fn operators_none() {
        let segs = split_on_operators("cargo test");
        assert_eq!(segs, vec![Segment::Command("cargo test".to_string())]);
    }

    #[test]
    fn operators_logical_and() {
        let segs = split_on_operators("a && b");
        assert_eq!(
            segs,
            vec![
                Segment::Command("a ".to_string()),
                Segment::Operator("&&".to_string()),
                Segment::Command(" b".to_string()),
            ]
        );
    }

    #[test]
    fn operators_logical_or() {
        let segs = split_on_operators("a || b");
        assert_eq!(
            segs,
            vec![
                Segment::Command("a ".to_string()),
                Segment::Operator("||".to_string()),
                Segment::Command(" b".to_string()),
            ]
        );
    }

    #[test]
    fn operators_semicolon() {
        let segs = split_on_operators("a; b");
        assert_eq!(
            segs,
            vec![
                Segment::Command("a".to_string()),
                Segment::Operator(";".to_string()),
                Segment::Command(" b".to_string()),
            ]
        );
    }

    #[test]
    fn operators_trailing_amp() {
        let segs = split_on_operators("a &");
        assert_eq!(
            segs,
            vec![
                Segment::Command("a ".to_string()),
                Segment::Operator("&".to_string()),
            ]
        );
    }

    #[test]
    fn operators_inside_quotes_ignored() {
        let segs = split_on_operators("a 'x && y' z");
        assert_eq!(
            segs,
            vec![Segment::Command("a 'x && y' z".to_string())]
        );
    }

    #[test]
    fn operators_inside_double_quotes_ignored() {
        let segs = split_on_operators(r#"a "x && y" z"#);
        assert_eq!(
            segs,
            vec![Segment::Command(r#"a "x && y" z"#.to_string())]
        );
    }

    #[test]
    fn operators_chained() {
        let segs = split_on_operators("a && b || c");
        assert_eq!(segs.len(), 5);
        assert!(matches!(segs[0], Segment::Command(_)));
        assert!(matches!(segs[1], Segment::Operator(_)));
        assert!(matches!(segs[2], Segment::Command(_)));
        assert!(matches!(segs[3], Segment::Operator(_)));
        assert!(matches!(segs[4], Segment::Command(_)));
    }

    #[test]
    fn operators_pipe_not_split() {
        let segs = split_on_operators("a | b");
        assert_eq!(segs, vec![Segment::Command("a | b".to_string())]);
    }

    #[test]
    fn operators_redirect_2_amp_1_not_split() {
        let segs = split_on_operators("cargo test 2>&1");
        assert_eq!(segs, vec![Segment::Command("cargo test 2>&1".to_string())]);
    }

    #[test]
    fn operators_redirect_amp_gt_not_split() {
        let segs = split_on_operators("cargo build &> log");
        assert_eq!(segs, vec![Segment::Command("cargo build &> log".to_string())]);
    }

    #[test]
    fn operators_redirect_lt_amp_not_split() {
        let segs = split_on_operators("cmd <&3");
        assert_eq!(segs, vec![Segment::Command("cmd <&3".to_string())]);
    }

    #[test]
    fn operators_redirect_with_chain_after() {
        let segs = split_on_operators("cargo test 2>&1 && echo done");
        assert_eq!(segs.len(), 3);
        if let Segment::Command(c) = &segs[0] {
            assert!(c.contains("2>&1"));
        } else {
            panic!("first segment should be command");
        }
        assert_eq!(segs[1], Segment::Operator("&&".to_string()));
    }

    #[test]
    fn operators_redirect_with_pipe_after() {
        let segs = split_on_operators("cargo test 2>&1");
        assert_eq!(segs.len(), 1);
    }

    #[test]
    fn tokenize_simple() {
        assert_eq!(tokenize("cargo test"), vec!["cargo", "test"]);
    }

    #[test]
    fn tokenize_single_quotes() {
        assert_eq!(tokenize("echo 'hello world'"), vec!["echo", "hello world"]);
    }

    #[test]
    fn tokenize_double_quotes() {
        assert_eq!(tokenize(r#"echo "hello world""#), vec!["echo", "hello world"]);
    }

    #[test]
    fn tokenize_escaped_space() {
        assert_eq!(tokenize(r"echo a\ b"), vec!["echo", "a b"]);
    }

    #[test]
    fn tokenize_empty_string_arg() {
        assert_eq!(tokenize("echo ''"), vec!["echo", ""]);
    }

    #[test]
    fn tokenize_collapses_whitespace() {
        assert_eq!(tokenize("a    b\t\tc"), vec!["a", "b", "c"]);
    }

    #[test]
    fn tokenize_trailing_whitespace() {
        assert_eq!(tokenize("a b   "), vec!["a", "b"]);
    }

    #[test]
    fn tokenize_leading_whitespace() {
        assert_eq!(tokenize("   a b"), vec!["a", "b"]);
    }

    #[test]
    fn parse_segment_simple() {
        let (env, head, args) = parse_segment("cargo test -p mem").unwrap();
        assert_eq!(env, "");
        assert_eq!(head, "cargo");
        assert_eq!(args, vec!["test", "-p", "mem"]);
    }

    #[test]
    fn parse_segment_with_env() {
        let (env, head, args) = parse_segment("RUST_LOG=debug cargo test").unwrap();
        assert_eq!(env, "RUST_LOG=debug ");
        assert_eq!(head, "cargo");
        assert_eq!(args, vec!["test"]);
    }

    #[test]
    fn parse_segment_empty_returns_none() {
        assert!(parse_segment("").is_none());
    }

    #[test]
    fn parse_segment_only_env_returns_none() {
        assert!(parse_segment("FOO=bar").is_none());
    }

    #[test]
    fn parse_segment_quoted_args() {
        let (env, head, args) = parse_segment(r#"git log --grep "hello world""#).unwrap();
        assert_eq!(env, "");
        assert_eq!(head, "git");
        assert_eq!(args, vec!["log", "--grep", "hello world"]);
    }
}
