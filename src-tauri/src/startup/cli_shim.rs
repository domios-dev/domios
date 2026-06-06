//! GUI binary forwards CLI-shaped argv to the bundled CLI. Without this,
//! single_instance silently consumes the dup launch and the args vanish —
//! the symptom in repo issue #51.

use std::path::PathBuf;

const KNOWN_SUBCOMMANDS: &[&str] = &[
    "identify",
    "capabilities",
    "workspace",
    "pane",
    "get-output",
    "send-text",
    "browser",
    "notify",
    "notifications",
    "keepawake",
    "theme",
    "agent",
    "cron",
    "update",
    "config",
    "bridge",
    "mesh",
    "mem",
    "sessions",
    "--help",
    "-h",
    "--version",
    "-V",
    // Global flags from cli/src/args/cli.rs that may precede a subcommand.
    // Without these, `codeterm --json mem search foo` would be silently
    // swallowed by the GUI binary on macOS (regression of #51).
    "--json",
    "--mesh",
];

pub fn forward_to_cli_if_invoked() {
    let mut args = std::env::args();
    let _self = args.next();
    let first = match args.next() {
        Some(s) => s,
        None => return,
    };

    if !is_cli_invocation(&first) {
        return;
    }

    let cli_path = match locate_cli_binary() {
        Some(p) => p,
        None => {
            eprintln!(
                "codeterm: this binary is the GUI app, not the CLI. \
                 Install the CLI by launching CodeTerm at least once \
                 (it auto-installs to ~/.codeterm/bin/codeterm and symlinks \
                 ~/.local/bin/codeterm), then re-run."
            );
            std::process::exit(2);
        }
    };

    exec_cli(cli_path);
}

fn is_cli_invocation(arg: &str) -> bool {
    let normalized = arg.trim();
    KNOWN_SUBCOMMANDS.iter().any(|c| *c == normalized)
}

fn locate_cli_binary() -> Option<PathBuf> {
    let bin_name = if cfg!(windows) {
        "codeterm.exe"
    } else {
        "codeterm"
    };
    let home = dirs::home_dir()?;

    let candidates = [
        home.join(".codeterm").join("bin").join(bin_name),
        home.join(".codeterm-dev").join("bin").join(bin_name),
    ];

    let self_exe = std::env::current_exe().ok();
    for c in candidates {
        if !c.exists() {
            continue;
        }
        // Refuse to exec ourselves — would loop forever.
        if let Some(ref s) = self_exe {
            if same_path(s, &c) {
                continue;
            }
        }
        return Some(c);
    }
    None
}

fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    let ca = std::fs::canonicalize(a).ok();
    let cb = std::fs::canonicalize(b).ok();
    match (ca, cb) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

#[cfg(unix)]
fn exec_cli(cli_path: PathBuf) -> ! {
    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new(&cli_path);
    cmd.args(std::env::args().skip(1));
    let err = cmd.exec();
    eprintln!("codeterm: exec {} failed: {}", cli_path.display(), err);
    std::process::exit(2);
}

#[cfg(windows)]
fn exec_cli(cli_path: PathBuf) -> ! {
    let status = std::process::Command::new(&cli_path)
        .args(std::env::args().skip(1))
        .status();
    match status {
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("codeterm: spawn {} failed: {}", cli_path.display(), e);
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_known_subcommands() {
        for name in [
            "agent",
            "mem",
            "mesh",
            "workspace",
            "pane",
            "send-text",
            "get-output",
            "--help",
            "-h",
            "--version",
            "-V",
        ] {
            assert!(
                is_cli_invocation(name),
                "expected '{}' to be a CLI invocation",
                name
            );
        }
    }

    #[test]
    fn ignores_non_subcommand_args() {
        for name in [
            "",
            "-psn_0_12345",
            "--gui-only-flag",
            "random",
            "/path/to/file",
        ] {
            assert!(
                !is_cli_invocation(name),
                "expected '{}' to NOT be a CLI invocation",
                name
            );
        }
    }

    #[test]
    fn detects_all_clap_top_level_subcommands() {
        // Mirror Commands enum in cli/src/args/cli.rs. Adding a new top-level
        // subcommand without updating KNOWN_SUBCOMMANDS regresses #51.
        for name in [
            "identify",
            "capabilities",
            "workspace",
            "pane",
            "get-output",
            "send-text",
            "browser",
            "notify",
            "notifications",
            "keepawake",
            "theme",
            "agent",
            "cron",
            "update",
            "config",
            "bridge",
            "mesh",
            "mem",
            "sessions",
        ] {
            assert!(
                is_cli_invocation(name),
                "expected top-level '{}' to be a CLI invocation",
                name
            );
        }
    }

    #[test]
    fn detects_global_flags_before_subcommand() {
        // `codeterm --json mem search foo` and `codeterm --mesh peer mem ls`:
        // the first arg is a global flag, not a subcommand. Without these
        // entries the GUI binary silently swallows the launch.
        for name in ["--json", "--mesh"] {
            assert!(
                is_cli_invocation(name),
                "expected global flag '{}' to forward as CLI",
                name
            );
        }
    }

    #[test]
    fn same_path_handles_missing_files() {
        let a = std::path::Path::new("/nonexistent/path/a");
        let b = std::path::Path::new("/nonexistent/path/a");
        assert!(same_path(a, b));
        let c = std::path::Path::new("/nonexistent/path/b");
        assert!(!same_path(a, c));
    }
}
