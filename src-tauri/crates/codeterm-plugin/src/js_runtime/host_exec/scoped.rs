use super::*;
pub(in crate::js_runtime) fn start_scoped_json_job<F>(
    plugin_id: &str,
    scope: &str,
    deadline: Instant,
    cancel: Arc<dyn Fn() + Send + Sync>,
    timeout_result: serde_json::Value,
    work: F,
) -> Result<String, String>
where
    F: FnOnce() -> String + Send + 'static,
{
    let job_id = start_exec_job(plugin_id)?;
    let mut failure = timeout_result.clone();
    failure["error"] = serde_json::Value::String(
        if failure["requestMayHaveStarted"] == true {
            "transport_unconfirmed"
        } else {
            "unavailable"
        }
        .into(),
    );
    let installed = {
        let mut jobs = EXEC_JOBS.lock();
        if let Some(job) = jobs.get_mut(&job_id) {
            job.scope = Some(scope.into());
            job.deadline = Some(deadline);
            job.cancel_hook = Some(cancel);
            job.timeout_result = Some(timeout_result);
            true
        } else {
            false
        }
    };
    if !installed {
        return Err("host job revoked before admission".into());
    }
    spawn_json_worker(job_id.clone(), move || {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
            .unwrap_or_else(|_| failure.to_string())
    });
    Ok(job_id)
}
pub(in crate::js_runtime) fn abort_scoped_json_job(plugin: &str, scope: &str, id: &str) -> bool {
    let hook = EXEC_JOBS
        .lock()
        .get(id)
        .filter(|job| job.plugin_id == plugin && job.scope.as_deref() == Some(scope))
        .and_then(|job| job.cancel_hook.clone());
    if let Some(hook) = hook {
        hook();
        true
    } else {
        false
    }
}
pub(in crate::js_runtime) fn drop_scoped_json_jobs(plugin: &str, scope: &str) {
    let ids = EXEC_JOBS
        .lock()
        .iter()
        .filter(|(_, job)| job.plugin_id == plugin && job.scope.as_deref() == Some(scope))
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    for id in ids {
        close(plugin, &id);
    }
}

pub(super) fn timeout_result(plugin: &str, id: &str) -> Option<String> {
    let (done, result) = EXEC_JOBS
        .lock()
        .get(id)
        .filter(|job| job.plugin_id == plugin)
        .map(|job| (job.done, job.timeout_result.clone()))?;
    if done {
        return Some(poll(plugin, id));
    }
    let result = result?;
    close(plugin, id);
    serde_json::to_string(&result).ok()
}

#[cfg(test)]
mod tests;
