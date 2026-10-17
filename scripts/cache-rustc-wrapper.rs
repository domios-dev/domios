// Caller: scripts/cache-rustc-wrapper.mjs; scripts/cache-policy.sh
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::{Command, ExitCode};

const BRIDGE: &str = "CODETERM_CACHE_COMPILER_BRIDGE";
const SETTINGS: [&str; 2] = ["CARGO_TARGET_DIR", "CARGO_INCREMENTAL"];

fn forward(mut command: Command) -> ExitCode {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let error = command.exec();
        eprintln!("compiler cache wrapper: {error}");
        ExitCode::FAILURE
    }
    #[cfg(not(unix))]
    {
        match command.status() {
            Ok(status) => std::process::exit(status.code().unwrap_or(1)),
            Err(error) => {
                eprintln!("compiler cache wrapper: {error}");
                ExitCode::FAILURE
            }
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let executable = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("compiler cache wrapper: {error}");
            return ExitCode::FAILURE;
        }
    };
    let compiler = match std::fs::read_to_string(executable.with_extension("compiler")) {
        Ok(path) => match std::fs::canonicalize(PathBuf::from(path)) {
            Ok(path) => path,
            Err(error) => {
                eprintln!("compiler cache wrapper: {error}");
                return ExitCode::FAILURE;
            }
        },
        Err(error) => {
            eprintln!("compiler cache wrapper: {error}");
            return ExitCode::FAILURE;
        }
    };

    if std::env::var_os(BRIDGE).as_deref() == Some(OsStr::new("1")) {
        // Restore driver settings for rustc: env! dependencies must still enter sccache's key.
        let mut command = Command::new(compiler);
        command.args(args).env_remove(BRIDGE);
        for name in SETTINGS {
            let saved = format!("CODETERM_CACHE_SAVED_{name}");
            let present = format!("CODETERM_CACHE_PRESENT_{name}");
            if std::env::var_os(&present).as_deref() == Some(OsStr::new("1")) {
                command.env(name, std::env::var_os(&saved).unwrap_or_default());
            } else {
                command.env_remove(name);
            }
            command.env_remove(saved).env_remove(present);
        }
        return forward(command);
    }

    let Some(requested) = args.first() else {
        eprintln!("compiler cache wrapper: expected a compiler command");
        return ExitCode::FAILURE;
    };
    if std::fs::canonicalize(requested).ok().as_ref() != Some(&compiler) {
        let mut command = Command::new(requested);
        command.args(&args[1..]);
        return forward(command);
    }

    let mut command = Command::new("sccache");
    command.arg(executable).args(&args[1..]).env(BRIDGE, "1");
    for name in SETTINGS {
        let saved = format!("CODETERM_CACHE_SAVED_{name}");
        let present = format!("CODETERM_CACHE_PRESENT_{name}");
        if let Some(value) = std::env::var_os(name) {
            command.env(saved, value).env(present, "1");
        } else {
            command.env_remove(saved).env(present, "0");
        }
        command.env_remove(name);
    }
    forward(command)
}
