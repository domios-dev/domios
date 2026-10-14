pub use codeterm_util::process_util::*;

#[cfg(test)]
mod spawn_audit_tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    #[test]
    fn daemon_windows_spawns_use_no_window_helper() {
        let root = PathBuf::from(crate::test_support::fixture_manifest_dir()).join("src");
        let mut failures = Vec::new();
        audit_dir(&root, &root, &mut failures);
        assert!(
            failures.is_empty(),
            "Windows daemon subprocesses must call process_util::no_window_* unless they are visible UI openers or non-Windows-only.\n{}",
            failures.join("\n")
        );
    }

    fn audit_dir(root: &Path, dir: &Path, failures: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if !skip_dir(root, &path) {
                    audit_dir(root, &path, failures);
                }
                continue;
            }
            if path.extension().and_then(|s| s.to_str()) == Some("rs") && !skip_file(root, &path) {
                audit_file(root, &path, failures);
            }
        }
    }

    fn audit_file(root: &Path, path: &Path, failures: &mut Vec<String>) {
        let Ok(source) = fs::read_to_string(path) else {
            return;
        };
        let lines: Vec<&str> = source.lines().collect();
        for (idx, line) in lines.iter().enumerate() {
            if !line.contains("Command::new(") && !line.contains("std::process::Command::new(") {
                continue;
            }
            let before = window(&lines, idx.saturating_sub(8), idx + 1).join("\n");
            let after = window(&lines, idx, idx + 40).join("\n");
            if after.contains("process_util::no_window_std")
                || after.contains("process_util::no_window_tokio")
                || after.contains("provider_update::ProcessTree::prepare")
                || before.contains("target_os = \"macos\"")
                || before.contains("target_os = \"linux\"")
                || before.contains("not(windows)")
                || allowed_visible_or_unix_spawn(line)
            {
                continue;
            }
            failures.push(format!(
                "{}:{}: {}",
                path.strip_prefix(root)
                    .unwrap_or(path)
                    .display()
                    .to_string()
                    .replace('\\', "/"),
                idx + 1,
                line.trim()
            ));
        }
    }

    fn window<'a>(lines: &'a [&str], start: usize, end: usize) -> &'a [&'a str] {
        &lines[start.min(lines.len())..end.min(lines.len())]
    }

    fn skip_dir(root: &Path, path: &Path) -> bool {
        let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy();
        matches!(rel.as_ref(), "bin" | "pty")
    }

    fn skip_file(root: &Path, path: &Path) -> bool {
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        rel == "process_util.rs" || rel.ends_with("/tests.rs") || rel == "keepawake/linux.rs"
    }

    fn allowed_visible_or_unix_spawn(line: &str) -> bool {
        [
            "\"explorer\"",
            "\"open\"",
            "\"xdg-open\"",
            "\"osascript\"",
            "\"xclip\"",
            "\"ps\"",
            "\"kill\"",
            "\"/bin/sh\"",
            "\"launchctl\"",
            "\"systemctl\"",
            "\"/usr/bin/id\"",
        ]
        .iter()
        .any(|needle| line.contains(needle))
    }
}
