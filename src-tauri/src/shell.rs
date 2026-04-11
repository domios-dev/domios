//! Platform-aware shell syntax helpers.
//!
//! Single source of truth for bash vs PowerShell differences.
//! Used by agent spawn (orchestrator) and hook generation (commands).

/// Shell syntax primitives for the current platform.
pub struct ShellSyntax;

impl ShellSyntax {
    pub const IS_WINDOWS: bool = cfg!(target_os = "windows");

    /// Statement to unset the CLAUDECODE env var.
    pub const UNSET_CLAUDECODE: &str = if Self::IS_WINDOWS {
        "Remove-Item Env:CLAUDECODE -ErrorAction SilentlyContinue;"
    } else {
        "unset CLAUDECODE &&"
    };

    /// Null device path.
    pub const NULL_DEV: &str = if Self::IS_WINDOWS { "NUL" } else { "/dev/null" };

    /// Write `task` to a temp file, return the shell expression that reads it inline.
    pub fn task_file_expr(task: &str) -> String {
        let (path, _) = Self::write_task_temp(task);
        if Self::IS_WINDOWS {
            format!("(Get-Content '{}' -Raw -Encoding UTF8)", path)
        } else {
            format!("\"$(cat {})\"", path)
        }
    }

    /// Write `task` to a temp file and return a shell prefix that pipes the
    /// file contents into the *next* command via stdin, plus the path.
    ///
    /// Used for headless agent panes (mem workers, cron) where the task can
    /// contain `--flag` tokens. The classic `task_file_expr` inline subexpression
    /// gets re-split by PowerShell when passed to a native command, so tokens
    /// like `--area` get lifted into the child's argv. The pipe form sidesteps
    /// argv splitting entirely — the child reads stdin instead.
    ///
    /// The returned prefix ends with a trailing space and `|` so the caller
    /// can directly concatenate the binary name + flags after it. Example:
    /// `"Get-Content 'C:\\...\\.ct-task-abcd' -Raw -Encoding UTF8 |"`.
    pub fn task_file_stdin_pipe(task: &str) -> (String, String) {
        let (path, _) = Self::write_task_temp(task);
        let prefix = if Self::IS_WINDOWS {
            format!("Get-Content '{}' -Raw -Encoding UTF8 |", path)
        } else {
            format!("cat '{}' |", path)
        };
        (prefix, path)
    }

    /// Write task content to a temp file, returning (path, short_id).
    /// On Windows, prepends a UTF-8 BOM so PowerShell reads it as UTF-8.
    fn write_task_temp(task: &str) -> (String, String) {
        let short_id = uuid::Uuid::new_v4().to_string()[..8].to_string();
        let path = Self::temp_path(&format!(".ct-task-{}", short_id));
        if Self::IS_WINDOWS {
            let mut content = Vec::with_capacity(3 + task.len());
            content.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
            content.extend_from_slice(task.as_bytes());
            let _ = std::fs::write(&path, content);
        } else {
            let _ = std::fs::write(&path, task);
        }
        (path, short_id)
    }

    /// Quote a string for the current shell.
    pub fn quote(s: &str) -> String {
        if Self::IS_WINDOWS {
            // PowerShell: double internal single quotes
            format!("'{}'", s.replace('\'', "''"))
        } else {
            // bash/zsh: end-escape-reopen
            format!("'{}'", s.replace('\'', "'\\''"))
        }
    }

    /// Platform temp file path (no trailing separator issues).
    fn temp_path(name: &str) -> String {
        if Self::IS_WINDOWS {
            let tmp = std::env::temp_dir();
            let base = tmp.to_string_lossy();
            let base = base.trim_end_matches('\\');
            format!("{}\\{}", base, name)
        } else {
            format!("/tmp/{}", name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_file_expr_writes_file_and_returns_inline_subexpr() {
        let expr = ShellSyntax::task_file_expr("hello world");
        if ShellSyntax::IS_WINDOWS {
            assert!(expr.starts_with("(Get-Content "));
            assert!(expr.ends_with(" -Raw -Encoding UTF8)"));
        } else {
            assert!(expr.starts_with("\"$(cat "));
            assert!(expr.ends_with(")\""));
        }
        assert!(expr.contains(".ct-task-"));
    }

    #[test]
    fn task_file_stdin_pipe_returns_pipe_prefix_and_writes_content() {
        let task = "codeterm mem save --kind fact --area codeterm \"body with --flag tokens\"";
        let (prefix, path) = ShellSyntax::task_file_stdin_pipe(task);
        assert!(prefix.ends_with('|'), "prefix must end with pipe char: {:?}", prefix);
        if ShellSyntax::IS_WINDOWS {
            assert!(prefix.starts_with("Get-Content '"));
            assert!(prefix.contains(" -Raw -Encoding UTF8 |"));
        } else {
            assert!(prefix.starts_with("cat '"));
            assert!(prefix.contains(" |"));
        }
        assert!(path.contains(".ct-task-"));

        // Content was actually written to disk and matches.
        let bytes = std::fs::read(&path).expect("temp task file written");
        let body = if ShellSyntax::IS_WINDOWS {
            // strip BOM
            assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]), "expected UTF-8 BOM on Windows");
            String::from_utf8(bytes[3..].to_vec()).expect("utf8")
        } else {
            String::from_utf8(bytes).expect("utf8")
        };
        assert_eq!(body, task);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn task_file_stdin_pipe_handles_multiline_and_quotes() {
        let task = "line one\nline 'two' has quotes\nline three --flag value";
        let (prefix, path) = ShellSyntax::task_file_stdin_pipe(task);
        assert!(prefix.ends_with('|'));
        let bytes = std::fs::read(&path).expect("temp task file written");
        let body = if ShellSyntax::IS_WINDOWS {
            String::from_utf8(bytes[3..].to_vec()).unwrap()
        } else {
            String::from_utf8(bytes).unwrap()
        };
        assert_eq!(body, task);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn task_file_stdin_pipe_path_is_quoted() {
        let (prefix, _) = ShellSyntax::task_file_stdin_pipe("x");
        // Path is wrapped in single quotes for both shells.
        let single_quotes = prefix.matches('\'').count();
        assert!(single_quotes >= 2, "path should be single-quoted: {:?}", prefix);
    }
}
