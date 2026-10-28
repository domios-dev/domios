// Caller: scripts/cache-rustc-wrapper.mjs; scripts/cache-policy.sh
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::{Command, ExitCode};

const BRIDGE: &str = "CODETERM_CACHE_COMPILER_BRIDGE";
const REMAP: &str = "CODETERM_CACHE_REMAP_GENERATED";
const POLICY: &str = "CARGO_CODETERM_CACHE_OUTPUT_POLICY";
const OUTPUT_PREFIX: &str = env!("CODETERM_CACHE_OUTPUT_PREFIX");
const CACHE_CLIENT: &str = env!("CODETERM_CACHE_CLIENT");
const SETTINGS: [&str; 3] = [
    "CARGO_TARGET_DIR",
    "CARGO_BUILD_BUILD_DIR",
    "CARGO_INCREMENTAL",
];

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
    if std::env::var_os(BRIDGE).is_none()
        && args
            .first()
            .is_none_or(|arg| arg.to_string_lossy().starts_with('-'))
    {
        let mut command = Command::new(CACHE_CLIENT);
        command.args(args);
        return forward(command);
    }
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
        if std::env::var_os(REMAP).as_deref() == Some(OsStr::new("1")) {
            let target = std::env::var_os("CODETERM_CACHE_SAVED_CARGO_TARGET_DIR").unwrap();
            let output = std::env::var_os("OUT_DIR").unwrap();
            let relative = std::path::Path::new(&output).strip_prefix(&target).unwrap();
            let mut mapping = OsString::from("--remap-path-prefix=");
            mapping.push(&output);
            mapping.push("=");
            mapping.push(std::path::Path::new(OUTPUT_PREFIX).join(relative));
            command.arg(mapping).env_remove(POLICY);
        }
        command.env_remove(REMAP);
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
        let rust = args.iter().any(|arg| arg == "--crate-name")
            || args.iter().take(2).any(|arg| {
                let name = std::path::Path::new(arg).file_stem().unwrap_or_default();
                let name = name.to_string_lossy();
                name.contains("rustc") || name.contains("clippy")
            });
        let mut command = Command::new(if rust {
            requested
        } else {
            OsStr::new(CACHE_CLIENT)
        });
        command.args(if rust { &args[1..] } else { &args });
        return forward(command);
    }

    let mut command = Command::new(CACHE_CLIENT);
    command.arg(executable).args(&args[1..]).env(BRIDGE, "1");
    let generated = std::env::var_os("CARGO_TARGET_DIR")
        .zip(std::env::var_os("OUT_DIR"))
        .and_then(|(target, output)| {
            let relative = std::path::Path::new(&output).strip_prefix(&target).ok()?;
            if output.to_str().is_some_and(|path| !path.contains('=')) {
                Some(relative.to_path_buf())
            } else {
                None
            }
        });
    if let Some(relative) = generated {
        command.env(REMAP, "1").env(
            POLICY,
            format!("generated-v1:{OUTPUT_PREFIX}:{}", relative.display()),
        );
    } else {
        command.env_remove(REMAP);
    }
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
