use crate::client::SocketClient;
use crate::MemCalibrationAction;

pub(crate) fn handle_mem_calibration(
    client: &mut SocketClient,
    action: MemCalibrationAction,
) -> Result<serde_json::Value, String> {
    match action {
        MemCalibrationAction::Log {
            since,
            limit,
            count,
        } => client.call(
            "mem.calibration.log",
            Some(serde_json::json!({ "since": since, "limit": limit, "count": count })),
        ),
        MemCalibrationAction::Sweep { force } => client.call(
            "mem.calibration.sweep",
            Some(serde_json::json!({ "force": force })),
        ),
        MemCalibrationAction::Stale { dry_run, limit } => client.call(
            "mem.calibration.stale",
            Some(serde_json::json!({ "dry_run": dry_run, "limit": limit })),
        ),
    }
}
