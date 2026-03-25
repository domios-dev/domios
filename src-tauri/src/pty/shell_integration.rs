//! Invisible shell integration: OSC 7 CWD tracking via env vars.
//!
//! Instead of injecting visible commands into the terminal after spawn,
//! we set up env vars at PTY creation time so the shell sources our hooks
//! silently during its normal startup.

use portable_pty::CommandBuilder;
use std::path::PathBuf;

/// Data dir where shell integration scripts live.
fn integration_dir() -> PathBuf {
    let dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".codeterm")
        .join("shell-integration");
    dir
}

/// Ensure shell integration scripts exist on disk.
/// Called once at app startup.
pub fn ensure_scripts() {
    let dir = integration_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("Failed to create shell integration dir: {}", e);
        return;
    }

    // ── zsh integration ──────────────────────────────────────────
    // ZDOTDIR trick: this .zshrc sources the user's real config then adds our hook
    let zshrc = dir.join(".zshrc");
    let zshrc_content = r#"# CodeTerm shell integration — do not edit
# Restore original ZDOTDIR and source user config
if [[ -n "$CODETERM_ORIG_ZDOTDIR" ]]; then
  ZDOTDIR="$CODETERM_ORIG_ZDOTDIR"
  unset CODETERM_ORIG_ZDOTDIR
else
  ZDOTDIR="$HOME"
fi

# Source all standard zsh startup files from original ZDOTDIR
[[ -f "$ZDOTDIR/.zshenv" ]]  && source "$ZDOTDIR/.zshenv"
[[ -f "$ZDOTDIR/.zprofile" ]] && source "$ZDOTDIR/.zprofile"
[[ -f "$ZDOTDIR/.zshrc" ]]   && source "$ZDOTDIR/.zshrc"

# OSC 7 CWD tracking hook (invisible)
__ct_osc7() { printf '\033]7;file://%s%s\033\\' "${HOST:-$(hostname)}" "$PWD" }
autoload -Uz add-zsh-hook 2>/dev/null
add-zsh-hook precmd __ct_osc7
"#;
    let _ = std::fs::write(&zshrc, zshrc_content);

    // Also create .zshenv to handle non-interactive shells
    let zshenv = dir.join(".zshenv");
    let zshenv_content = r#"# CodeTerm — pass through to user's .zshenv
if [[ -n "$CODETERM_ORIG_ZDOTDIR" ]]; then
  [[ -f "$CODETERM_ORIG_ZDOTDIR/.zshenv" ]] && source "$CODETERM_ORIG_ZDOTDIR/.zshenv"
elif [[ -f "$HOME/.zshenv" ]]; then
  source "$HOME/.zshenv"
fi
"#;
    let _ = std::fs::write(&zshenv, zshenv_content);

    // ── bash integration ─────────────────────────────────────────
    // Sourced via BASH_ENV for non-login, or via --rcfile trick
    let bashrc = dir.join("bashrc");
    let bashrc_content = r#"# CodeTerm shell integration — do not edit
# Source user's bashrc first
[[ -f "$HOME/.bashrc" ]] && source "$HOME/.bashrc"

# OSC 7 CWD tracking hook
__ct_osc7() { printf '\033]7;file://%s%s\033\\' "$(hostname)" "$PWD"; }
PROMPT_COMMAND="__ct_osc7${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
"#;
    let _ = std::fs::write(&bashrc, bashrc_content);

    log::info!("Shell integration scripts ready at {}", dir.display());
}

/// Apply shell integration env vars to a CommandBuilder based on detected shell.
pub fn apply_env(cmd: &mut CommandBuilder, shell: &str) {
    let dir = integration_dir();

    #[cfg(not(target_os = "windows"))]
    {
        if shell.contains("zsh") {
            // Save original ZDOTDIR (if any) and point to our integration dir
            let orig = std::env::var("ZDOTDIR").unwrap_or_default();
            if !orig.is_empty() {
                cmd.env("CODETERM_ORIG_ZDOTDIR", &orig);
            } else {
                // Explicitly set to HOME so our .zshrc knows where to look
                cmd.env(
                    "CODETERM_ORIG_ZDOTDIR",
                    std::env::var("HOME").unwrap_or_else(|_| "/".to_string()),
                );
            }
            cmd.env("ZDOTDIR", dir.to_string_lossy().as_ref());
        } else if shell.contains("bash") {
            // For bash: set PROMPT_COMMAND directly as env var for login shells,
            // and BASH_ENV for non-interactive subshells
            let bashrc_path = dir.join("bashrc");
            if bashrc_path.exists() {
                cmd.env("BASH_ENV", bashrc_path.to_string_lossy().as_ref());
                // For interactive login bash, BASH_ENV isn't sourced.
                // We use ENV as a fallback marker; the --rcfile approach won't
                // work with login shells. Instead, set PROMPT_COMMAND directly.
                cmd.env(
                    "PROMPT_COMMAND",
                    r#"printf '\033]7;file://%s%s\033\\' "$(hostname)" "$PWD""#,
                );
            }
        } else {
            // Other POSIX shells: set PROMPT_COMMAND equivalent via ENV
            cmd.env(
                "PROMPT_COMMAND",
                r#"printf '\033]7;file://%s%s\033\\' "$(hostname)" "$PWD""#,
            );
        }
    }

    #[cfg(target_os = "windows")]
    {
        // PowerShell integration is handled differently — we override the prompt
        // function via PSModulePath or profile injection.
        // For now, we set a marker env var; the frontend sends the prompt override.
        let _ = (cmd, shell, dir); // suppress unused warnings
    }
}
