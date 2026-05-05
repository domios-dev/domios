//! Resolve the CodeTerm CLI binary path provider hook installers
//! embed in their hook commands. Bundled by the auto-installer at
//! `~/.codeterm/bin/codeterm` (Windows: `codeterm.exe`); falls back to
//! `codeterm` on PATH if the home dir can't be resolved.
//!
//! Always returns forward slashes — backslashes break git-bash quoting
//! and would otherwise need per-OS handling at every install site.
//! Quoting around the path is each installer's responsibility (Claude
//! single-quotes for git-bash on Windows; Codex double-quotes because
//! cmd.exe treats single quotes as literals).

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
