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
        let short_id = &uuid::Uuid::new_v4().to_string()[..8];
        let path = Self::temp_path(&format!(".ct-task-{}", short_id));
        if Self::IS_WINDOWS {
            // Write UTF-8 BOM + content so PowerShell reads it as UTF-8
            let mut content = Vec::with_capacity(3 + task.len());
            content.extend_from_slice(&[0xEF, 0xBB, 0xBF]); // UTF-8 BOM
            content.extend_from_slice(task.as_bytes());
            let _ = std::fs::write(&path, content);
            format!("(Get-Content '{}' -Raw -Encoding UTF8)", path)
        } else {
            let _ = std::fs::write(&path, task);
            format!("\"$(cat {})\"", path)
        }
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
