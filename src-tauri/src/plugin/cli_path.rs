//! Resolve the CodeTerm CLI binary path provider hook installers
//! embed in their hook commands. Bundled by the auto-installer at
//! `~/.codeterm/bin/codeterm` (Windows: `codeterm.exe`); falls back to
//! `codeterm` on PATH if the home dir can't be resolved.
//!
//! Always returns forward slashes — `'C:/Users/.../codeterm.exe'` is the
//! one form that survives both git-bash (Claude on Windows) and cmd.exe
//! (Codex on Windows). Backslashes break git-bash quoting; OS-native
//! separators on Unix are forward-slashed already so this is a no-op.

pub fn codeterm_cli_path() -> String {
    if let Some(home) = dirs::home_dir() {
        let exe = if cfg!(windows) { "codeterm.exe" } else { "codeterm" };
        let p = home.join(".codeterm").join("bin").join(exe);
        if let Some(s) = p.to_str() {
            return s.replace('\\', "/");
        }
    }
    "codeterm".to_string()
}
