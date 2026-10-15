use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use codeterm_plugin::agent_task::{
    validate_input, validate_result, AgentTaskRequest, AgentTaskResult, AgentTaskStatus,
};
use codeterm_plugin::chatbackend_provider::ChatBackendProvider;

pub fn run(
    provider: Arc<dyn ChatBackendProvider>,
    request: AgentTaskRequest,
) -> Result<AgentTaskResult, String> {
    validate_input(&request.input)?;
    if !provider.supports_agent_tasks() {
        return Err("agent provider capability unavailable".into());
    }
    let run_id = request.input.run_id.clone();
    let deadline = request.deadline;
    let cancellation = request.cancellation.clone();
    if cancellation.is_cancelled() || Instant::now() >= deadline {
        return Ok(AgentTaskResult {
            status: if cancellation.is_cancelled() {
                AgentTaskStatus::Cancelled
            } else {
                AgentTaskStatus::DeadlineExceeded
            },
            output: serde_json::Value::Null,
            error: Some("agent task refused before dispatch".into()),
        });
    }
    let worker_signal = cancellation.clone();
    let worker_provider = provider.clone();
    let (complete, result) = mpsc::channel();
    std::thread::spawn(move || {
        let value = worker_provider
            .run_agent_task(&request)
            .and_then(validate_result);
        let _ = complete.send(value);
        worker_signal.notify_change();
    });
    loop {
        let generation = cancellation.generation();
        if cancellation.is_cancelled() || Instant::now() >= deadline {
            let status = if cancellation.is_cancelled() {
                AgentTaskStatus::Cancelled
            } else {
                AgentTaskStatus::DeadlineExceeded
            };
            cancellation.cancel();
            let abort_provider = provider.clone();
            std::thread::spawn(move || {
                let _ = abort_provider.abort_agent_task(&run_id);
            });
            return Ok(AgentTaskResult {
                status,
                output: serde_json::Value::Null,
                error: Some("agent task aborted; unfinished tool effects are unconfirmed".into()),
            });
        }
        if let Ok(value) = result.try_recv() {
            return value;
        }
        cancellation.wait_for_change(
            generation,
            deadline.saturating_duration_since(Instant::now()),
        );
    }
}

pub fn effective_deadline(stage_ms: u64, remaining_ms: Option<u64>, now: Instant) -> Instant {
    now + Duration::from_millis(stage_ms.min(remaining_ms.unwrap_or(stage_ms)))
}

pub fn result_fields(result: &AgentTaskResult) -> (bool, bool) {
    (
        result.status == AgentTaskStatus::Done,
        matches!(
            result.status,
            AgentTaskStatus::Cancelled | AgentTaskStatus::DeadlineExceeded
        ),
    )
}

pub fn persist_result(run_id: &str, index: usize, output: &str) -> Result<String, String> {
    if !run_id.bytes().all(|b| b.is_ascii_alphanumeric()) || output.len() > 65536 {
        return Err("invalid agent result persistence request".into());
    }
    let dir = codeterm_paths::data_dir().join("blueprint-agent-results");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{run_id}-{index}.json"));
    codeterm_util::atomic_write::write_atomic(&path, output).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests;
