use std::path::PathBuf;

// An extracted nextest archive moves the binary; env! keeps the build machine's path.
pub fn codeterm_bin() -> PathBuf {
    std::env::var_os("NEXTEST_BIN_EXE_codeterm")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_codeterm")))
}
