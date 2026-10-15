use codeterm_service_core::startup::{
    repair_user_path_with, user_path_repair_report, SystemUserPathStore,
};

const REPAIR_ARG: &str = "repair-user-path";

/// `CodeTerm repair-user-path` — never automatic, never part of startup. The
/// GUI binary has no console on Windows, so the report is also written to a
/// file whose path the caller is told.
pub(crate) fn run_user_path_repair_if_invoked() {
    if std::env::args().nth(1).as_deref() != Some(REPAIR_ARG) {
        return;
    }
    let (report, code) = match repair_user_path_with(&SystemUserPathStore, &std::env::temp_dir()) {
        Ok(removed) => (user_path_repair_report(&removed), 0),
        Err(error) => (format!("Domios user PATH repair failed: {error}\n"), 1),
    };
    let report_path = std::env::temp_dir().join("codeterm-user-path-repair.txt");
    let _ = std::fs::write(&report_path, &report);
    use std::io::Write;
    let _ = writeln!(
        std::io::stdout(),
        "{report}Report written to {}",
        report_path.display()
    );
    std::process::exit(code);
}
