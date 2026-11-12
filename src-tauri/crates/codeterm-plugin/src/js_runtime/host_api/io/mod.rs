use std::sync::Arc;

use rquickjs::{function::Func, Object};
use serde_json::{json, Value};

use super::super::{
    host_exec,
    provider_observation::ProviderObservation,
    vm::{operation, VmError},
};
use super::{current_file_lock_resources, FileLockCallScope, PluginSandbox};

mod admission;
mod filesystem;
mod protocol;
mod service;

use protocol::Request;

pub(in crate::js_runtime) fn export_protocol_contracts(
    config: &ts_rs::Config,
) -> Result<(), Box<dyn std::error::Error>> {
    use ts_rs::TS;
    Request::export_all(config)?;
    Ok(())
}

pub(super) struct Failure {
    message: String,
    range: bool,
}
impl Failure {
    fn range(message: String) -> Self {
        Self {
            message,
            range: true,
        }
    }
}
impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self {
            message,
            range: false,
        }
    }
}
impl From<&str> for Failure {
    fn from(message: &str) -> Self {
        message.to_string().into()
    }
}

pub(in crate::js_runtime) fn install(
    host: &Object<'_>,
    sandbox: &Arc<PluginSandbox>,
    observation: &Arc<ProviderObservation>,
) -> Result<(), VmError> {
    let sandbox = sandbox.clone();
    let observation = observation.clone();
    host.set(
        "ioStart",
        Func::new(move |request: String| -> String {
            match start(&sandbox, &observation, &request) {
                Ok(job) => json!({ "jobId": job }).to_string(),
                Err(error) => json!({ "error": error }).to_string(),
            }
        }),
    )
    .map_err(|error| VmError::Script(format!("set ioStart: {error}")))
}

pub(super) fn enforce_jobs_only(host: &Object<'_>) -> Result<(), VmError> {
    for name in [
        "surfaceCall",
        "surfaceInvalidate",
        "transferStart",
        "transferAbort",
        "transferBindInput",
        "execPoll",
        "execClose",
        "fetchStreamPoll",
        "fetchStreamClose",
        "providerVersionJson",
        "fileExists",
        "isSymlink",
        "readFile",
        "readFileHead",
        "readFileTail",
        "writeFile",
        "writeFileAtomic",
        "canonicalPath",
        "projectWriteAllowed",
        "jsonBooleanRecord",
        "lockFile",
        "unlockFile",
        "removeFile",
        "makeDirs",
        "readDirJson",
        "listChildDirsJson",
        "walkDirJson",
        "writeTempFile",
        "sqliteQueryAllJson",
        "sqliteQueryStart",
        "exec",
        "execStart",
        "credentialPublic",
        "fetch",
        "fetchStart",
        "fetchStream",
        "secretGet",
        "secretSet",
        "secretDelete",
        "workerStart",
        "workerPoll",
        "progressRaw",
        "workspaceEnsure",
        "workspaceTabs",
        "agentSpawn",
        "agentSend",
        "agentPoll",
        "agentReap",
        "memSearch",
        "analyticsQuery",
        "graphEventEmit",
        "receiverTargetGet",
        "receiverTargetClear",
        "receiverStatusReport",
        "localRecordingInputs",
        "localRecordingSelect",
        "localRecordingSavedSelection",
        "localRecordingStatus",
        "localRecordingSetEnabled",
    ] {
        host.set(
            name,
            Func::new(move |ctx: rquickjs::Ctx| -> rquickjs::Result<()> {
                Err(rquickjs::Exception::throw_type(
                    &ctx,
                    &format!("{name} requires a host.io job"),
                ))
            }),
        )
        .map_err(|error| VmError::Script(format!("host I/O contract: {error}")))?;
    }
    Ok(())
}

fn start(
    sandbox: &Arc<PluginSandbox>,
    observation: &Arc<ProviderObservation>,
    request: &str,
) -> Result<String, String> {
    if request.len() > 2 * 1024 * 1024 {
        return Err("host I/O request size limit".into());
    }
    let request: Request =
        serde_json::from_str(request).map_err(|_| "invalid host I/O request".to_string())?;
    let read_only = request.is_read_only();
    let effects_allowed = codeterm_decision::physical::effects_allowed();
    let operation = operation::current().ok_or("host I/O requires an active invocation")?;
    operation.check().map_err(|error| error.to_string())?;
    let resources =
        current_file_lock_resources().ok_or("host I/O invocation resources unavailable")?;
    let job_owner = operation.job();
    #[cfg(test)]
    let completion = match &request {
        Request::TestGate { completed, .. } => Some(completed.clone()),
        _ => None,
    };
    let sandbox = sandbox.clone();
    let observation = observation.clone();
    let observer = codeterm_decision::physical::current();
    let host_env = host_exec::current_host_env();
    let artifacts = codeterm_util::task_files::current_capture();
    let plugin = sandbox.plugin_id().to_string();
    let root = crate::surface_invocation::root_identity();
    let surface = crate::surface_invocation::current(&plugin);
    let view = crate::view_host::current_view_call_context(&plugin);
    let cancel_operation = operation.clone();
    let owner_operation = operation.clone();
    let owner_root = root.clone();
    let admitted = plugin.clone();
    let ticket = admission::ticket();
    let job = host_exec::start_admitted_scoped_json_job(
        &plugin,
        &root,
        operation.deadline,
        Arc::new(move || cancel_operation.revoke()),
        json!({"error":"io_unconfirmed", "requestMayHaveStarted":!read_only && effects_allowed}),
        move |launch| admission::submit(&admitted, ticket, launch),
        move |permit: admission::Permit| {
            let encoded = {
                let _permit = permit;
                let _job_owner = job_owner;
                let _host_env = host_exec::host_env_scope(host_env);
                let _observer = observer.map(codeterm_decision::physical::RequestScope::enter);
                let _files = FileLockCallScope::attach(resources);
                let _operation = operation::Scope::enter(operation.clone());
                let work = || {
                    codeterm_util::task_files::with_capture(artifacts, || {
                        operation
                            .check()
                            .map_err(|error| Failure::from(error.to_string()))
                            .and_then(|_| {
                                with_surface(surface, || {
                                    execute(request, &sandbox, &observation, &operation)
                                })
                            })
                    })
                };
                let result = match view {
                    Some(context) => crate::view_host::with_view_call_context(
                        sandbox.plugin_id(),
                        &context,
                        || Ok(work()),
                    )
                    .map_err(|error| Failure::from(error.to_string()))
                    .and_then(|result| result),
                    None => work(),
                };
                match result {
                    Ok(value) => json!({"value":value}).to_string(),
                    Err(error) => json!({"error":error.message,"kind":if error.range {"range"} else {"native"}}).to_string(),
                }
            };
            #[cfg(test)]
            if let Some(completed) = completion {
                super::super::call_gate::open(&completed);
            }
            encoded
        },
    )?;
    // Cancellation, close, deadline close or call exit revokes the call; a still-queued
    // job then drops its captures at once instead of waiting for capacity.
    owner_operation.on_revoke(Box::new(move || admission::withdraw(ticket)));
    host_exec::allow_observation(&plugin, &job);
    host_exec::bind_invocation(&plugin, &job, owner_root);
    owner_operation.register_job(job.clone());
    Ok(job)
}

fn with_surface(
    surface: Option<crate::surface_invocation::SurfaceInvocationContext>,
    work: impl FnOnce() -> Result<Value, Failure>,
) -> Result<Value, Failure> {
    let Some(surface) = surface else {
        return work();
    };
    let _root = crate::surface_invocation::RootScope::enter(
        &surface.plugin,
        Some(surface.registration.clone()),
    );
    let ticket = crate::surface_invocation::issue(surface)?;
    if !crate::surface_invocation::begin(&ticket) {
        return Err("host I/O surface scope unavailable".into());
    }
    let result = work();
    crate::surface_invocation::end(&ticket, false);
    result
}

fn execute(
    request: Request,
    sandbox: &Arc<PluginSandbox>,
    observation: &ProviderObservation,
    operation: &operation::Context,
) -> Result<Value, Failure> {
    use Request::*;
    match request {
        ExecPoll { job } => serde_json::from_str(&host_exec::poll_owned(sandbox.plugin_id(), &job))
            .map_err(|error| Failure::from(error.to_string())),
        ExecClose { job } => {
            operation.begin_effect().map_err(|error| error.to_string())?;
            host_exec::close_owned(sandbox.plugin_id(), &job);
            Ok(Value::Null)
        }
        FetchStreamPoll { job } => {
            operation.begin_effect().map_err(|error| error.to_string())?;
            serde_json::from_str(&super::super::host_net::stream_poll(sandbox.plugin_id(), &job))
                .map_err(|error| Failure::from(error.to_string()))
        }
        FetchStreamClose { job } => {
            operation.begin_effect().map_err(|error| error.to_string())?;
            super::super::host_net::stream_close(sandbox.plugin_id(), &job);
            Ok(Value::Null)
        }
        FetchStreamStart { options } => {
            let prepared = super::prepare_fetch(sandbox, &options.to_string())?;
            operation.begin_effect().map_err(|error| error.to_string())?;
            serde_json::from_str(&super::super::host_net::stream(sandbox.plugin_id(), &prepared))
                .map_err(|error| Failure::from(error.to_string()))
        }
        ExecStart { options } => {
            let options = options.to_string();
            let binary = host_exec::bin_of(&options).unwrap_or_default();
            if !sandbox.can_exec(&binary) {
                return Err("subprocess permission denied".into());
            }
            operation
                .begin_effect()
                .map_err(|error| error.to_string())?;
            serde_json::from_str(&host_exec::start_with_host_env(
                sandbox.plugin_id(),
                &options,
                &host_exec::current_host_env(),
                sandbox.clone(),
            ))
            .map_err(|error| Failure::from(error.to_string()))
        }
        FetchStart { options } => {
            let prepared = super::prepare_fetch(sandbox, &options.to_string())?;
            operation
                .begin_effect()
                .map_err(|error| error.to_string())?;
            serde_json::from_str(&super::super::host_net::start(
                sandbox.plugin_id(),
                &prepared,
            ))
            .map_err(|error| Failure::from(error.to_string()))
        }
        ProviderVersion => {
            Ok(serde_json::to_value(observation.version(sandbox)).unwrap_or(Value::Null))
        }
        CredentialPublic { id } => Ok(super::super::host_credentials::public_json(
            sandbox.credentials(),
            &id,
        )
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or(Value::Null)),
        SqliteQuery {
            db,
            sql,
            params,
            nullable,
        } => super::sqlite::query_value(
            sandbox,
            observation,
            &db,
            &sql,
            params.as_ref().map(Value::to_string).as_deref(),
            nullable,
        )
        .map_err(Failure::range),
        #[cfg(test)]
        TestGate {
            entered, release, ..
        } => {
            super::super::call_gate::open(&entered);
            super::super::call_gate::wait(&release);
            operation.check().map_err(|error| error.to_string())?;
            Ok(Value::Null)
        }
        request if filesystem::handles(&request) => {
            filesystem::execute(request, sandbox, operation)
        }
        request => service::execute(request, sandbox, operation),
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod lifecycle_tests;
