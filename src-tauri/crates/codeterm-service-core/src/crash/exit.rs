pub use codeterm_crash::crash::exit::{
    execute_exit_plan, plan_run_exit, shutdown_state_persistence_result, ExitPlan,
};

#[cfg(test)]
mod tests;

#[derive(Debug, PartialEq, Eq)]
#[cfg(test)]
enum ShutdownSidecarLogLevel {
    Info,
    Warn,
}

#[derive(Debug, PartialEq, Eq)]
#[cfg(test)]
struct ShutdownSidecarLogRecord {
    level: ShutdownSidecarLogLevel,
    message: String,
}

#[cfg(test)]
fn shutdown_sidecar_log_records(
    report: &crate::session::scrollback_sync::SidecarSyncReport,
) -> Vec<ShutdownSidecarLogRecord> {
    let mut records = Vec::with_capacity(report.written.len() + report.skipped.len() + 1);
    records.extend(
        report
            .written
            .iter()
            .map(|(tab_id, bytes)| ShutdownSidecarLogRecord {
                level: ShutdownSidecarLogLevel::Info,
                message: format!(
                    "[shutdown] scrollback sidecar tab={tab_id} result=written bytes={bytes}"
                ),
            }),
    );
    records.extend(
        report
            .skipped
            .iter()
            .map(|(tab_id, reason)| ShutdownSidecarLogRecord {
                level: ShutdownSidecarLogLevel::Warn,
                message: format!(
                    "[shutdown] scrollback sidecar tab={tab_id} result=skipped reason={reason}"
                ),
            }),
    );
    let level = if report.skipped.is_empty() {
        ShutdownSidecarLogLevel::Info
    } else {
        ShutdownSidecarLogLevel::Warn
    };
    let summary = if report.skipped.is_empty() {
        format!(
            "[shutdown] scrollback sidecars synchronized written={} skipped=0",
            report.written.len()
        )
    } else {
        format!(
            "[shutdown] scrollback sidecar synchronization incomplete written={} skipped={}",
            report.written.len(),
            report.skipped.len()
        )
    };
    records.push(ShutdownSidecarLogRecord {
        level,
        message: summary,
    });
    records
}

use crate::service::codex_rpc::exit::{begin, Shutdown};

pub struct SidecarShutdown(Shutdown);

pub fn begin_sidecar_shutdown() -> SidecarShutdown {
    SidecarShutdown(begin())
}

impl SidecarShutdown {
    pub fn wait(self) {
        self.0.wait();
    }
}
