#[cfg(target_os = "macos")]
pub use codeterm_crash::crash::macos_quit;
#[cfg(debug_assertions)]
pub use codeterm_crash::crash::trigger_test_crash;
pub use codeterm_crash::crash::{
    check_previous_crashes, dump_record, image_identity, install, lifecycle, record_clean_shutdown,
    record_startup, report, shutdown,
};

pub mod exit;
pub mod restore_report;
