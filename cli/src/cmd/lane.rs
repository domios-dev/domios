use serde_json::{json, Value};

use crate::args::lane::{LaneAction, LaneSubjectArgs};
use crate::client::RpcClient;
use crate::cmd::plan::call_plan_rpc;

pub fn handle_lane(client: &mut dyn RpcClient, json_mode: bool, action: LaneAction) {
    match run_lane(client, action) {
        Ok(value) if json_mode => crate::cmd::print::print_json(&value),
        Ok(value) => print_human(&value),
        Err(error) => crate::cmd::print::handle_result(json_mode, Err(error)),
    }
}

fn run_lane(client: &mut dyn RpcClient, action: LaneAction) -> Result<Value, String> {
    let (method, LaneSubjectArgs { subject, plan }) = match action {
        LaneAction::Check(args) => ("lane.check", args),
        LaneAction::Diagnostics(args) => ("lane.diagnostics", args),
    };
    let mut params = json!({ "subject": subject });
    if let Some(plan) = plan {
        params["plan_id"] = json!(plan);
    }
    call_plan_rpc(client, method, params)
}

fn print_human(value: &Value) {
    println!(
        "lane diagnostics {}/{}",
        value.get("plan_id").and_then(Value::as_str).unwrap_or("?"),
        value.get("task_id").and_then(Value::as_str).unwrap_or("?")
    );
    let Some(diagnostics) = value.get("diagnostics") else {
        return;
    };
    println!(
        "status={} errors={} warnings={} info={}",
        diagnostics
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("?"),
        diagnostics
            .get("error_count")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        diagnostics
            .get("warning_count")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        diagnostics
            .get("info_count")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    );
    if let Some(findings) = diagnostics.get("findings").and_then(Value::as_array) {
        for finding in findings {
            let location = match (
                finding.get("file").and_then(Value::as_str),
                finding.get("line").and_then(Value::as_u64),
                finding.get("column").and_then(Value::as_u64),
            ) {
                (Some(file), Some(line), Some(column)) => format!("{file}:{line}:{column}"),
                (Some(file), Some(line), None) => format!("{file}:{line}"),
                (Some(file), None, _) => file.to_string(),
                _ => "<repo>".into(),
            };
            println!(
                "{} {} {}: {}",
                finding
                    .get("severity")
                    .and_then(Value::as_str)
                    .unwrap_or("info"),
                finding
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or("diagnostics"),
                location,
                finding
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_diagnostics_human_location_uses_file_line_column() {
        let value = json!({
            "plan_id": "plan",
            "task_id": "task",
            "diagnostics": {
                "status": "complete",
                "error_count": 1,
                "warning_count": 0,
                "info_count": 0,
                "findings": [{
                    "tool": "tsc",
                    "severity": "error",
                    "file": "src/app.ts",
                    "line": 4,
                    "column": 8,
                    "message": "type mismatch"
                }]
            }
        });
        assert_eq!(value["diagnostics"]["findings"][0]["line"], 4);
        assert_eq!(value["diagnostics"]["findings"][0]["column"], 8);
    }
}
