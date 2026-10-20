//! Platform-aware shell syntax helpers.
//!
//! Single source of truth for bash vs PowerShell differences.
//! Used by agent spawn (orchestrator) and hook generation (commands).

/// Shell syntax primitives for the current platform.
pub struct ShellSyntax;

impl ShellSyntax {
    pub const IS_WINDOWS: bool = cfg!(target_os = "windows");

    // Windows: unset CLAUDECODE, force UTF-8, pin Standard argv-passing (default on pwsh 7.3+; explicit so older pwsh / 5.1 also pass a `"$(…)"` result as a single argv element).
    pub const UNSET_CLAUDECODE: &str = if Self::IS_WINDOWS {
        "Remove-Item Env:CLAUDECODE -ErrorAction SilentlyContinue; $OutputEncoding = [Console]::OutputEncoding = [Console]::InputEncoding = [System.Text.Encoding]::UTF8; $PSNativeCommandArgumentPassing = 'Standard';"
    } else {
        "unset CLAUDECODE &&"
    };

    /// Null device path.
    pub const NULL_DEV: &str = if Self::IS_WINDOWS { "NUL" } else { "/dev/null" };

    /// Write `task` to a temp file, return the shell expression that reads it inline.
    ///
    /// Windows: wrap in `"$(…)"` so PowerShell builds one string and hands it to
    /// the native command as a single argv element. Bare `(Get-Content -Raw)` was
    /// argv-split by PS native-command argument passing — any `--flag` token in
    /// the task body got reparsed as a CLI option (claude's headless path already
    /// had the same fix via `task_file_stdin_pipe`).
    pub fn task_file_expr(task: &str) -> String {
        let (path, _) = Self::write_task_temp(task);
        if Self::IS_WINDOWS {
            format!("\"$(Get-Content '{}' -Raw -Encoding UTF8)\"", path)
        } else {
            format!("\"$(cat {})\"", path)
        }
    }

    // Returns (`Get-Content '<path>' -Raw -Encoding UTF8 |` or `cat '<path>' |`, path).
    pub fn task_file_stdin_pipe(task: &str) -> (String, String) {
        let (path, _) = Self::write_task_temp(task);
        let prefix = if Self::IS_WINDOWS {
            format!("Get-Content '{}' -Raw -Encoding UTF8 |", path)
        } else {
            format!("cat '{}' |", path)
        };
        (prefix, path)
    }

    pub fn task_file_path(task: &str) -> String {
        let (path, _) = Self::write_task_temp(task);
        path
    }

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
            format!("'{}'", s.replace('\'', "''"))
        } else {
            format!("'{}'", s.replace('\'', "'\\''"))
        }
    }

    /// Platform temp file path (no trailing separator issues).
    fn temp_path(name: &str) -> String {
        let tmp = std::env::temp_dir();
        let base = tmp.to_string_lossy();
        if Self::IS_WINDOWS {
            let base = base.trim_end_matches('\\');
            format!("{}\\{}", base, name)
        } else {
            let base = base.trim_end_matches('/');
            format!("{}/{}", base, name)
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
            assert!(expr.starts_with("\"$(Get-Content "));
            assert!(expr.ends_with(" -Raw -Encoding UTF8)\""));
        } else {
            assert!(expr.starts_with("\"$(cat "));
            assert!(expr.ends_with(")\""));
        }
        assert!(expr.contains(".ct-task-"));
    }

    #[test]
    fn task_file_expr_windows_wraps_in_double_quotes_for_argv_safety() {
        if !ShellSyntax::IS_WINDOWS {
            return;
        }
        let body = "do something --yes --extra-flag\nmultiline";
        let expr = ShellSyntax::task_file_expr(body);
        assert!(expr.starts_with('"') && expr.ends_with('"'),
            "Windows form must be wrapped in PS double quotes so the native command sees a single argv element: {expr}");
    }

    #[test]
    fn unset_claudecode_pins_standard_argv_passing_on_windows() {
        if !ShellSyntax::IS_WINDOWS {
            return;
        }
        // even on older pwsh versions whose default is Legacy.
        assert!(
            ShellSyntax::UNSET_CLAUDECODE.contains("$PSNativeCommandArgumentPassing = 'Standard'"),
            "preamble must pin Standard argv passing: {}",
            ShellSyntax::UNSET_CLAUDECODE
        );
    }

    #[test]
    fn task_file_persists_problematic_body_unchanged() {
        // Body containing patterns that historically broke argv passing or got re-interpolated by the launching shell: `-f` flag, $vars, backticks, embedded quotes, multiline, unicode.
        let body = "step --foo bar\n`whoami`\n$HOME contains 'quotes' and \"more\"\nemoji 🦀 ok";
        let (prefix, path) = ShellSyntax::task_file_stdin_pipe(body);
        assert!(prefix.ends_with('|'));
        let bytes = std::fs::read(&path).expect("temp task file written");
        let written = if ShellSyntax::IS_WINDOWS {
            assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
            String::from_utf8(bytes[3..].to_vec()).unwrap()
        } else {
            String::from_utf8(bytes).unwrap()
        };
        assert_eq!(written, body, "task body must round-trip byte-exact");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn task_file_stdin_pipe_returns_pipe_prefix_and_writes_content() {
        let task = "codeterm mem save --kind fact --area codeterm \"body with --flag tokens\"";
        let (prefix, path) = ShellSyntax::task_file_stdin_pipe(task);
        assert!(
            prefix.ends_with('|'),
            "prefix must end with pipe char: {:?}",
            prefix
        );
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
            assert!(
                bytes.starts_with(&[0xEF, 0xBB, 0xBF]),
                "expected UTF-8 BOM on Windows"
            );
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
        assert!(
            single_quotes >= 2,
            "path should be single-quoted: {:?}",
            prefix
        );
    }

    #[test]
    fn task_file_path_writes_content_without_shell_expansion() {
        let body = "large task body\n--flag $env:VALUE `quoted`";
        let path = ShellSyntax::task_file_path(body);
        let bytes = std::fs::read(&path).expect("temp task file written");
        let written = if ShellSyntax::IS_WINDOWS {
            assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
            String::from_utf8(bytes[3..].to_vec()).unwrap()
        } else {
            String::from_utf8(bytes).unwrap()
        };
        assert_eq!(written, body);
        let _ = std::fs::remove_file(&path);
    }
}
