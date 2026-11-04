use super::*;
use std::sync::Arc;

struct CancelOnDrop(Arc<operation::Context>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.revoke();
    }
}

pub(in crate::js_runtime) async fn call_json_async<
    R: serde::de::DeserializeOwned + Send + 'static,
>(
    vm: Arc<PluginVm>,
    method: &'static str,
    args: Vec<serde_json::Value>,
) -> Result<R, VmError> {
    with_blocking_operation(vm, move |vm, context| {
        call_in_operation(vm, method, &args, context)
    })
    .await
}

pub(in crate::js_runtime) async fn with_blocking_operation<R, F>(
    vm: Arc<PluginVm>,
    work: F,
) -> Result<R, VmError>
where
    R: Send + 'static,
    F: FnOnce(&PluginVm, Arc<operation::Context>) -> Result<R, VmError> + Send + 'static,
{
    let operation = vm.admission.enter(Some(default_call_interrupt(&vm)))?;
    let context = operation.context.clone();
    let _cancel = CancelOnDrop(context.clone());
    let observer = codeterm_decision::physical::current();
    let host_env = super::super::host_exec::current_host_env();
    let view = crate::view_host::current_view_call_context(vm.plugin_id());
    let failed_context = context.clone();
    tokio::task::spawn_blocking(move || {
        let _operation = operation;
        let _observer = observer.map(codeterm_decision::physical::RequestScope::enter);
        let _host_env = super::super::host_exec::host_env_scope(host_env);
        context.check()?;
        let call = || work(&vm, context);
        match view {
            Some(view) => {
                crate::view_host::with_view_call_context(vm.plugin_id(), &view, || Ok(call()))
                    .map_err(|error| VmError::Script(error.to_string()))?
            }
            None => call(),
        }
    })
    .await
    .map_err(|_| match failed_context.check() {
        Err(VmError::IoUnconfirmed) => VmError::IoUnconfirmed,
        _ => VmError::Script("plugin blocking worker failed".into()),
    })?
}
