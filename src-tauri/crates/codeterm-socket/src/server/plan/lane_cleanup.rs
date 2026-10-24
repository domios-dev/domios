use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::{Condvar, Mutex};
use serde_json::{json, Value};

use super::{emit_plan_changed, JsonRpcError, ServiceState};
use codeterm_service_core::plan::Plan;

#[derive(Default)]
struct Jobs(Mutex<Registry>);

#[derive(Default)]
struct Registry {
    next: u64,
    jobs: HashMap<String, Arc<Job>>,
}

struct Job {
    plan_id: String,
    confirm: bool,
    state: Mutex<Progress>,
    changed: Condvar,
}

#[derive(Default)]
struct Progress {
    events: Vec<Value>,
    result: Option<Result<Value, String>>,
}

fn error(message: impl Into<String>) -> JsonRpcError {
    JsonRpcError {
        code: -32003,
        message: message.into(),
        data: None,
    }
}

fn jobs(svc: &ServiceState) -> Result<Arc<Jobs>, JsonRpcError> {
    if let Some(jobs) = svc.host_extension::<Jobs>() {
        return Ok(jobs);
    }
    let registry = Arc::new(Jobs::default());
    if svc.register_host_extension(registry.clone()).is_ok() {
        return Ok(registry);
    }
    svc.host_extension::<Jobs>()
        .ok_or_else(|| error("lane cleanup registry unavailable"))
}

pub(super) fn start(
    svc: &ServiceState,
    plan: Plan,
    confirm: bool,
    fetch: bool,
    stream: bool,
) -> Result<Value, JsonRpcError> {
    let job = Arc::new(Job {
        plan_id: plan.id.clone(),
        confirm: confirm || fetch,
        state: Mutex::new(Progress::default()),
        changed: Condvar::new(),
    });
    let registry = jobs(svc)?;
    let id = {
        let mut registry = registry.0.lock();
        if (confirm || fetch)
            && registry.jobs.values().any(|job| {
                job.plan_id == plan.id && job.confirm && job.state.lock().result.is_none()
            })
        {
            return Err(error(
                "lane cleanup is already running for this plan; preview remains available",
            ));
        }
        if registry.jobs.len() >= 16 {
            registry
                .jobs
                .retain(|_, job| job.state.lock().result.is_none());
        }
        if registry.jobs.len() >= 16 {
            return Err(error("lane cleanup capacity is busy"));
        }
        registry.next += 1;
        let id = registry.next.to_string();
        registry.jobs.insert(id.clone(), job.clone());
        id
    };
    let service = svc.clone();
    service.rt.clone().spawn_blocking(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let lanes = codeterm_orchestration::lane_clean::clean_plan_lanes_with_progress(
                &service,
                &plan,
                confirm,
                fetch,
                |row, finished| {
                    job.state
                        .lock()
                        .events
                        .push(json!({"lane": row, "finished": finished}));
                    job.changed.notify_all();
                },
            )?;
            if confirm {
                emit_plan_changed(&service, &plan.id);
            }
            Ok(json!({"plan_id": plan.id, "dry_run": !confirm, "lanes": lanes}))
        }))
        .unwrap_or_else(|_| Err("lane cleanup worker panicked".into()));
        job.state.lock().result = Some(result);
        job.changed.notify_all();
    });
    if stream {
        Ok(json!({"job_id": id}))
    } else {
        let mut cursor = 0;
        loop {
            let update = next(svc, &id, cursor)?;
            cursor = update["cursor"].as_u64().unwrap_or(0) as usize;
            if update["done"] == true {
                return Ok(update["result"].clone());
            }
        }
    }
}

pub(super) fn next(svc: &ServiceState, id: &str, cursor: usize) -> Result<Value, JsonRpcError> {
    let registry = jobs(svc)?;
    let job = registry
        .0
        .lock()
        .jobs
        .get(id)
        .cloned()
        .ok_or_else(|| error("lane cleanup job not found"))?;
    let mut state = job.state.lock();
    if cursor > state.events.len() {
        return Err(error("invalid lane cleanup progress cursor"));
    }
    while cursor == state.events.len() && state.result.is_none() {
        job.changed.wait(&mut state);
    }
    let result = match &state.result {
        Some(Ok(value)) => Some(value.clone()),
        Some(Err(message)) => return Err(error(message.clone())),
        None => None,
    };
    let response = json!({
        "events": state.events[cursor..], "cursor": state.events.len(),
        "done": result.is_some(), "result": result,
    });
    drop(state);
    if response["done"] == true {
        registry.0.lock().jobs.remove(id);
    }
    Ok(response)
}

#[cfg(test)]
mod tests;
