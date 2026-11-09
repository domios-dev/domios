use super::*;
use std::sync::Arc;

thread_local! {
    static BLOCKING_WORKER: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn on_blocking_worker() -> bool {
    BLOCKING_WORKER.with(Cell::get)
}

/// Runs synchronous plugin calls on a Tokio blocking thread that the VM recognizes as
/// off-runtime, including under a current-thread runtime.
pub fn spawn_blocking<F, R>(work: F) -> tokio::task::JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let artifacts = codeterm_util::task_files::current_capture();
    tokio::task::spawn_blocking(move || {
        struct Restore(bool);
        impl Drop for Restore {
            fn drop(&mut self) {
                BLOCKING_WORKER.with(|worker| worker.set(self.0));
            }
        }
        let _restore = Restore(BLOCKING_WORKER.with(|worker| worker.replace(true)));
        codeterm_util::task_files::with_capture(artifacts, work)
    })
}

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
    let interrupt = default_call_interrupt(&vm);
    call_json_async_until(vm, method, args, interrupt).await
}

/// Like [`call_json_async`], bounded by `interrupt` instead of the standard call bound.
pub(in crate::js_runtime) async fn call_json_async_until<
    R: serde::de::DeserializeOwned + Send + 'static,
>(
    vm: Arc<PluginVm>,
    method: &'static str,
    args: Vec<serde_json::Value>,
    interrupt: (crate::converter::ConversionCancellation, std::time::Instant),
) -> Result<R, VmError> {
    with_blocking_operation_interruptible(vm, interrupt, move |vm, context| {
        call_in_operation(vm, method, &args, context)
    })
    .await
}

/// Runs synchronous plugin `work` on the blocking pool, with `interrupt` as the whole
/// operation's cancellation and deadline, host I/O jobs included.
pub(in crate::js_runtime) async fn with_blocking_operation_interruptible<R, F>(
    vm: Arc<PluginVm>,
    interrupt: (crate::converter::ConversionCancellation, std::time::Instant),
    work: F,
) -> Result<R, VmError>
where
    R: Send + 'static,
    F: FnOnce(&PluginVm, Arc<operation::Context>) -> Result<R, VmError> + Send + 'static,
{
    let _runtime = tokio::runtime::Handle::try_current()
        .map_err(|error| VmError::RuntimeInit(error.to_string()))?;
    let operation = vm.admission.enter(Some(interrupt))?;
    let context = operation.context.clone();
    let _cancel = CancelOnDrop(context.clone());
    let observer = codeterm_decision::physical::current();
    let host_env = super::super::host_exec::current_host_env();
    let view = crate::view_host::current_view_call_context(vm.plugin_id());
    let failed_context = context.clone();
    spawn_blocking(move || {
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
