use super::*;

pub(super) struct AwaitRequest {
    pub job: String,
    pub continuation: String,
}

pub(super) fn parse_await(result_json: &str) -> Option<AwaitRequest> {
    if !result_json.starts_with("{\"__ctAwait__\":") {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(result_json).ok()?;
    let outer = value.as_object()?;
    if outer.len() != 1 {
        return None;
    }
    let marker = outer.get("__ctAwait__")?.as_object()?;
    if marker.len() != 2 {
        return None;
    }
    Some(AwaitRequest {
        job: marker.get("job")?.as_str()?.into(),
        continuation: marker.get("k")?.as_str()?.into(),
    })
}

pub(super) fn validate(
    vm: &PluginVm,
    method: &str,
    pending: &AwaitRequest,
    deadline: Option<std::time::Instant>,
) -> Result<(), VmError> {
    if !crate::js_runtime::host_exec::await_ticket_owned(vm.plugin_id(), &pending.job) {
        return Err(VmError::Script(
            "await job is foreign, unavailable or denied".into(),
        ));
    }
    let inner = VmEntry::acquire(vm, method, deadline)?;
    inner._ctx.with(|ctx| {
        let validate: Function = ctx
            .globals()
            .get("__ct_await_validate__")
            .map_err(|_| VmError::Script("await validator unavailable".into()))?;
        let valid: bool = validate
            .call((pending.continuation.as_str(), pending.job.as_str()))
            .map_err(|_| VmError::Script("await validation failed".into()))?;
        if valid {
            Ok(())
        } else {
            Err(VmError::Script(
                "await ticket is foreign, stale or consumed".into(),
            ))
        }
    })
}

pub(super) fn discard(vm: &PluginVm, pending: &AwaitRequest) {
    let _scope =
        crate::surface_invocation::ResumeScope::enter(&pending.continuation, &pending.job, true);
    if let Some(inner) = vm.inner.try_lock() {
        inner._ctx.with(|ctx| {
            if let Ok(take) = ctx.globals().get::<_, Function>("__ct_await_take__") {
                let _ = take.call::<_, rquickjs::Value>((pending.continuation.as_str(),));
            }
        });
    }
}

pub(super) fn discard_root(vm: &PluginVm) {
    if let Some(inner) = vm.inner.try_lock() {
        inner._ctx.with(|ctx| {
            if let Ok(discard) = ctx
                .globals()
                .get::<_, Function>("__ct_await_discard_root__")
            {
                let _ = discard.call::<_, ()>(());
            }
        });
    }
}
