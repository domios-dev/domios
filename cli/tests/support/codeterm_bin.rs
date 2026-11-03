use std::path::PathBuf;
use std::process::Command;

pub fn command() -> Command {
    let mut command = Command::new(codeterm_bin());
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy().to_ascii_uppercase();
        if name == "CODETERM"
            || name == "DOMIOS"
            || name.starts_with("CODETERM_")
            || name.starts_with("DOMIOS_")
        {
            command.env_remove(key);
        }
    }
    command
}

// An extracted nextest archive moves the binary; env! keeps the build machine's path.
pub fn codeterm_bin() -> PathBuf {
    std::env::var_os("NEXTEST_BIN_EXE_codeterm")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_codeterm")))
}
