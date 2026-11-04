mod fs;
mod terminal;
#[cfg(test)]
mod tests;

use super::{
    engine::{self, Session},
    model::RuntimeFailure,
    process::Process,
};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::*;
use codeterm_types::pty::PendingPrompt;
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{Arc, LazyLock},
};

#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "method", content = "params")]
enum Request {
    #[serde(rename = "fs/read_text_file")]
    Read {
        #[serde(rename = "sessionId")]
        session_id: String,
        path: String,
        line: Option<usize>,
        limit: Option<usize>,
    },
    #[serde(rename = "fs/write_text_file")]
    Write {
        #[serde(rename = "sessionId")]
        session_id: String,
        path: String,
        content: String,
    },
    #[serde(rename = "terminal/create")]
    Create {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(flatten)]
        command: terminal::Command,
    },
    #[serde(rename = "terminal/output")]
    Output {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "terminalId")]
        terminal_id: String,
    },
    #[serde(rename = "terminal/wait_for_exit")]
    Wait {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "terminalId")]
        terminal_id: String,
    },
    #[serde(rename = "terminal/kill")]
    Kill {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "terminalId")]
        terminal_id: String,
    },
    #[serde(rename = "terminal/release")]
    Release {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "terminalId")]
        terminal_id: String,
    },
}
impl Request {
    fn session(&self) -> &str {
        match self {
            Self::Read { session_id, .. }
            | Self::Write { session_id, .. }
            | Self::Create { session_id, .. }
            | Self::Output { session_id, .. }
            | Self::Wait { session_id, .. }
            | Self::Kill { session_id, .. }
            | Self::Release { session_id, .. } => session_id,
        }
    }
}
struct Pending {
    request: Request,
    turn: String,
    generation: u64,
    env: BTreeMap<String, String>,
}
struct State {
    closed: bool,
    generation: u64,
    session: Option<String>,
    pending: HashMap<RpcId, Pending>,
    terminals: HashMap<String, Arc<terminal::Terminal>>,
}
struct Bridge {
    epoch: u64,
    nonce: String,
    root: PathBuf,
    root_handle: std::fs::File,
    capability: RuntimeClientBridges,
    state: Mutex<State>,
    inflight: Arc<tokio::sync::Semaphore>,
}
static BRIDGES: LazyLock<Mutex<HashMap<String, Arc<Bridge>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) fn install(
    context: &RuntimeContext,
    declared: Option<RuntimeClientBridges>,
) -> Result<Option<RuntimeClientBridges>, String> {
    let Some(capability) = declared else {
        return Ok(None);
    };
    if cfg!(not(unix)) {
        return Ok(None);
    }
    let root = codeterm_util::project_roots::canonical_root(&context.cwd)?;
    #[cfg(unix)]
    let root_handle = codeterm_blueprint_types::code_source::open_directory_no_follow(
        std::path::Path::new(&context.cwd),
    )
    .map_err(str::to_string)?;
    #[cfg(not(unix))]
    let root_handle =
        codeterm_blueprint_types::code_source::open_directory(&root).map_err(str::to_string)?;
    if dirs::home_dir().is_some_and(|home| home.canonicalize().ok().as_ref() == Some(&root)) {
        return Err("Bridge root cannot be home".into());
    }
    let mut bridges = BRIDGES.lock();
    if bridges.contains_key(&context.tab_id) {
        return Err("Previous client bridge was not retired".into());
    }
    bridges.insert(
        context.tab_id.clone(),
        Arc::new(Bridge {
            epoch: context.epoch,
            nonce: context.binding_nonce.clone(),
            root,
            root_handle,
            capability: capability.clone(),
            state: Mutex::new(State {
                closed: false,
                generation: 0,
                session: context.thread_id.clone(),
                pending: HashMap::new(),
                terminals: HashMap::new(),
            }),
            inflight: Arc::new(tokio::sync::Semaphore::new(64)),
        }),
    );
    Ok(Some(capability))
}
fn bridge(context: &RuntimeContext) -> Result<Arc<Bridge>, String> {
    BRIDGES
        .lock()
        .get(&context.tab_id)
        .filter(|bridge| bridge.epoch == context.epoch && bridge.nonce == context.binding_nonce)
        .cloned()
        .ok_or_else(|| "Client bridge epoch unavailable".into())
}
pub(super) fn method_supported(method: &str) -> bool {
    matches!(
        method,
        "fs/read_text_file"
            | "fs/write_text_file"
            | "terminal/create"
            | "terminal/output"
            | "terminal/wait_for_exit"
            | "terminal/kill"
            | "terminal/release"
    )
}
fn failure(message: String) -> RpcFailure {
    RpcFailure {
        code: -32602,
        message,
        data: None,
    }
}

fn prepare(
    svc: &ServiceContext,
    session: &Session,
    context: &RuntimeContext,
    bridge: &Bridge,
    request: &Request,
) -> Result<(Option<PendingPrompt>, BTreeMap<String, String>), String> {
    if context.thread_id.as_deref() != Some(request.session()) {
        return Err("Foreign client session".into());
    }
    let question = match request {
        Request::Read { path, line, .. } => {
            if !bridge.capability.fs_read || *line == Some(0) {
                return Err("Read denied".into());
            }
            let relative = fs::relative(&bridge.root, path)?;
            if fs::machine_secret(&relative) {
                return Err("Machine credential store denied".into());
            }
            fs::check(&bridge.root, &relative, false)?;
            if fs::secret(&relative) {
                Some(format!(
                    "Read sensitive workspace file: {}",
                    bridge.root.join(relative).display()
                ))
            } else {
                None
            }
        }
        Request::Write { path, content, .. } => {
            if content.len() > fs::MAX_BYTES {
                return Err("Write exceeds 1 MiB".into());
            }
            if !bridge.capability.fs_write {
                return Err("Write denied".into());
            }
            let relative = fs::relative(&bridge.root, path)?;
            if fs::machine_secret(&relative) {
                return Err("Machine credential store denied".into());
            }
            fs::check(&bridge.root, &relative, true)?;
            Some(format!(
                "Write workspace file: {}\n{}",
                bridge.root.join(relative).display(),
                content
            ))
        }
        Request::Create { command, .. } => {
            if !bridge.capability.terminal
                || command.command.is_empty()
                || command.command.len() > fs::MAX_PATH
                || command.command.chars().any(char::is_control)
                || command.args.len() > 128
                || command
                    .args
                    .iter()
                    .any(|arg| arg.len() > 16384 || arg.contains('\0'))
                || command
                    .output_byte_limit
                    .is_some_and(|limit| limit > fs::MAX_BYTES)
            {
                return Err("Terminal command exceeds host bounds".into());
            }
            let mut displayed = command.clone();
            displayed.cwd = Some(
                terminal_cwd(bridge, command)?
                    .to_string_lossy()
                    .into_owned(),
            );
            Some(format!(
                "Run workspace command: {}",
                serde_json::to_string(&displayed).map_err(|error| error.to_string())?
            ))
        }
        _ => {
            if !bridge.capability.terminal {
                return Err("Terminal denied".into());
            }
            None
        }
    };
    if question.is_some() {
        crate::service::execution_sandbox::validate_runtime_operation(
            svc,
            context,
            &RuntimeOperation::Answer {
                request_id: RpcId::Number(0),
                answer: PromptAnswer::Choice {
                    values: vec!["allow_once".into()],
                },
            },
        )?;
    }
    let env = if let Request::Create { command, .. } = request {
        let exclusions = session
            .adapter
            .lock()
            .as_ref()
            .and_then(|adapter| adapter.agent_runtime())
            .map(|cap| cap.inherited_environment_exclusions)
            .unwrap_or_default();
        terminal::environment(
            super::environment::compose(
                svc,
                &context.tab_id,
                &session.provider,
                &context.binding_nonce,
            ),
            &command.env,
            &exclusions,
        )?
    } else {
        BTreeMap::new()
    };
    let prompt = question.map(|question| serde_json::from_value(json!({"question":question,"options":[{"label":"Allow once","value":"allow_once"},{"label":"Reject","value":"reject_once"}]})).map_err(|error|error.to_string())).transpose()?;
    Ok((prompt, env))
}
fn terminal_cwd(bridge: &Bridge, command: &terminal::Command) -> Result<PathBuf, String> {
    let cwd = command
        .cwd
        .as_deref()
        .unwrap_or_else(|| bridge.root.to_str().unwrap_or(""));
    if PathBuf::from(cwd) == bridge.root {
        return Ok(bridge.root.clone());
    }
    let relative = fs::relative(&bridge.root, cwd)?;
    let mut path = bridge.root.clone();
    for part in relative.components() {
        path.push(part);
        let handle =
            codeterm_blueprint_types::code_source::open_directory(&path).map_err(str::to_string)?;
        if !handle.metadata().is_ok_and(|meta| meta.is_dir()) {
            return Err("Terminal cwd is not a directory".into());
        }
    }
    Ok(path)
}

pub(super) async fn handle(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    id: &RpcId,
    method: &str,
    params: &Value,
) -> Result<bool, RuntimeFailure> {
    if !method_supported(method) {
        return Ok(false);
    }
    let context = session.context.lock().clone();
    let bridge = match bridge(&context) {
        Ok(bridge) => bridge,
        Err(error) => {
            process.client.reply(id, Err(failure(error))).await?;
            return Ok(true);
        }
    };
    let permit = match bridge.inflight.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            process
                .client
                .reply(id, Err(failure("Client bridge capacity exceeded".into())))
                .await?;
            return Ok(true);
        }
    };
    let request = serde_json::from_value::<Request>(json!({"method":method,"params":params}))
        .map_err(|error| error.to_string());
    let request = match request {
        Ok(request) => request,
        Err(error) => {
            process.client.reply(id, Err(failure(error))).await?;
            return Ok(true);
        }
    };
    let generation = {
        let mut state = bridge.state.lock();
        if state.session.is_none() {
            state.session = context.thread_id.clone();
        }
        if state.closed || state.session.as_deref() != Some(request.session()) {
            None
        } else {
            Some(state.generation)
        }
    };
    let Some(generation) = generation else {
        process
            .client
            .reply(id, Err(failure("Client request scope expired".into())))
            .await?;
        return Ok(true);
    };
    let prepared = {
        let owner = svc.clone();
        let session = session.clone();
        let context = context.clone();
        let bridge = bridge.clone();
        let request = request.clone();
        tokio::task::spawn_blocking(move || prepare(&owner, &session, &context, &bridge, &request))
            .await
            .map_err(|error| RuntimeFailure::HookFailed(error.to_string()))?
    };
    let (prompt, env) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            process.client.reply(id, Err(failure(error))).await?;
            return Ok(true);
        }
    };
    if let Some(prompt) = prompt {
        let Some(turn) = context.active_turn_id.clone() else {
            process
                .client
                .reply(id, Err(failure("Approval requires an active turn".into())))
                .await?;
            return Ok(true);
        };
        let refused = {
            let mut state = bridge.state.lock();
            let mut prompts = session.prompts.lock();
            if state.closed
                || state.generation != generation
                || state.pending.len() >= 64
                || prompts.len() >= 64
                || prompts.contains_key(id)
            {
                true
            } else {
                state.pending.insert(
                    id.clone(),
                    Pending {
                        request,
                        turn,
                        generation,
                        env,
                    },
                );
                prompts.insert(
                    id.clone(),
                    engine::OutstandingRequest {
                        request: PendingRuntimeRequest {
                            request_id: id.clone(),
                            method: method.into(),
                            params: params.clone(),
                        },
                        prompt: Some(prompt.clone()),
                    },
                );
                false
            }
        };
        if refused {
            process
                .client
                .reply(id, Err(failure("Bridge request unavailable".into())))
                .await?;
        } else {
            super::prompt_answers::publish(svc, session, id.clone(), prompt);
        }
    } else {
        let owner = svc.clone();
        let session = session.clone();
        let process = process.clone();
        let id = id.clone();
        crate::async_runtime::spawn(async move {
            let _permit = permit;
            let result = execute(
                &owner, &session, &context, &bridge, generation, request, env,
            )
            .await
            .map_err(failure);
            let _ = process.client.reply(&id, result).await;
        });
    }
    Ok(true)
}

pub(super) async fn answer(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    id: &RpcId,
    answer: &PromptAnswer,
) -> Result<bool, RuntimeFailure> {
    let context = session.context.lock().clone();
    let Ok(bridge) = bridge(&context) else {
        return Ok(false);
    };
    let pending = bridge.state.lock().pending.remove(id);
    let Some(pending) = pending else {
        return Ok(false);
    };
    let original = session.prompts.lock().get(id).and_then(|entry| {
        serde_json::from_value::<Request>(
            json!({"method":entry.request.method,"params":entry.request.params}),
        )
        .ok()
    });
    let unchanged = original.as_ref() == Some(&pending.request);
    session.prompts.lock().remove(id);
    super::prompt_answers::resolve(svc, session, id);
    let granted = matches!(answer,PromptAnswer::Choice {values} if values == &["allow_once"]);
    let policy = crate::service::execution_sandbox::validate_runtime_operation(
        svc,
        &context,
        &RuntimeOperation::Answer {
            request_id: id.clone(),
            answer: answer.clone(),
        },
    );
    let result = if let Err(error) = policy {
        Err(error)
    } else if !unchanged {
        Err("Approval request changed".into())
    } else if !granted {
        Err("Client operation denied".into())
    } else if context.active_turn_id.as_deref() != Some(&pending.turn) {
        Err("Approval turn expired".into())
    } else {
        execute(
            svc,
            session,
            &context,
            &bridge,
            pending.generation,
            pending.request,
            pending.env,
        )
        .await
    };
    process.client.reply(id, result.map_err(failure)).await?;
    Ok(true)
}

async fn execute(
    svc: &ServiceContext,
    session: &Arc<Session>,
    context: &RuntimeContext,
    bridge: &Arc<Bridge>,
    generation: u64,
    request: Request,
    env: BTreeMap<String, String>,
) -> Result<Value, String> {
    engine::validate_binding(svc, session).map_err(|error| error.to_string())?;
    let current = session.context.lock().clone();
    if current.epoch != context.epoch
        || current.binding_nonce != context.binding_nonce
        || current.thread_id.as_deref() != Some(request.session())
    {
        return Err("Stale client request".into());
    }
    match request {
        Request::Read {
            path, line, limit, ..
        } => {
            let bridge = bridge.clone();
            tokio::task::spawn_blocking(move || {
                let state = bridge.state.lock();
                if state.closed || state.generation != generation {
                    return Err("Bridge closed".into());
                }
                let relative = fs::relative(&bridge.root, &path)?;
                fs::read(&bridge.root, &bridge.root_handle, &relative, line, limit)
                    .map(|content| json!({"content":content}))
            })
            .await
            .map_err(|error| error.to_string())?
        }
        Request::Write { path, content, .. } => {
            let bridge = bridge.clone();
            let session = session.clone();
            let expected = context.clone();
            tokio::task::spawn_blocking(move || {
                let state = bridge.state.lock();
                if state.closed || state.generation != generation {
                    return Err("Bridge closed".into());
                }
                let relative = fs::relative(&bridge.root, &path)?;
                fs::write(
                    &bridge.root,
                    &bridge.root_handle,
                    &relative,
                    &content,
                    |commit| {
                        let current = session.context.lock();
                        if current.epoch != expected.epoch
                            || current.binding_nonce != expected.binding_nonce
                            || current.thread_id != expected.thread_id
                            || current.active_turn_id != expected.active_turn_id
                        {
                            return Err("Write grant expired before commit".into());
                        }
                        commit()
                    },
                )?;
                Ok(json!({}))
            })
            .await
            .map_err(|error| error.to_string())?
        }
        Request::Create { command, .. } => {
            let bridge = bridge.clone();
            let session = session.clone();
            let expected = context.clone();
            tokio::task::spawn_blocking(move || {
                let mut state = bridge.state.lock();
                if state.closed || state.generation != generation || state.terminals.len() >= 8 {
                    return Err("Terminal capacity unavailable".into());
                }
                let current = session.context.lock();
                if current.epoch != expected.epoch
                    || current.binding_nonce != expected.binding_nonce
                    || current.thread_id != expected.thread_id
                    || current.active_turn_id != expected.active_turn_id
                {
                    return Err("Command grant expired before spawn".into());
                }
                fs::root_matches(&bridge.root, &bridge.root_handle)?;
                let cwd = terminal_cwd(&bridge, &command)?;
                let directory = fs::directory(
                    &bridge.root_handle,
                    cwd.strip_prefix(&bridge.root)
                        .map_err(|_| "Terminal cwd outside root")?,
                )?;
                let terminal = terminal::Terminal::spawn(&command, &cwd, directory, &env)?;
                let id = uuid::Uuid::new_v4().to_string();
                state.terminals.insert(id.clone(), terminal);
                Ok(json!({"terminalId":id}))
            })
            .await
            .map_err(|error| error.to_string())?
        }
        request => {
            let (id, wait, kill, release) = match request {
                Request::Output { terminal_id, .. } => (terminal_id, false, false, false),
                Request::Wait { terminal_id, .. } => (terminal_id, true, false, false),
                Request::Kill { terminal_id, .. } => (terminal_id, false, true, false),
                Request::Release { terminal_id, .. } => (terminal_id, false, true, true),
                _ => return Err("Invalid terminal request".into()),
            };
            let terminal = {
                let state = bridge.state.lock();
                if state.closed || state.generation != generation {
                    return Err("Bridge closed".into());
                }
                state
                    .terminals
                    .get(&id)
                    .cloned()
                    .ok_or("Foreign terminal handle")?
            };
            if kill {
                terminal.kill().await?;
            }
            if release {
                bridge.state.lock().terminals.remove(&id);
                return Ok(json!({}));
            }
            if wait {
                terminal.wait().await
            } else if kill {
                Ok(json!({}))
            } else {
                Ok(terminal.output())
            }
        }
    }
}

pub(super) async fn cancel(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    close: bool,
) -> Result<(), RuntimeFailure> {
    let context = session.context.lock().clone();
    let Ok(bridge) = bridge(&context) else {
        return Ok(());
    };
    let (pending, terminals) = {
        let mut state = bridge.state.lock();
        state.closed = true;
        state.generation += 1;
        (
            std::mem::take(&mut state.pending),
            std::mem::take(&mut state.terminals),
        )
    };
    let mut errors = Vec::new();
    for (id, _) in pending {
        if let Err(error) = process
            .client
            .reply(&id, Err(failure("Client operation cancelled".into())))
            .await
        {
            errors.push(error.to_string());
        }
        session.prompts.lock().remove(&id);
        super::prompt_answers::resolve(svc, session, &id);
    }
    for terminal in terminals.into_values() {
        if let Err(error) = terminal.kill().await {
            errors.push(error);
        }
    }
    if close {
        BRIDGES
            .lock()
            .retain(|tab, installed| tab != &context.tab_id || !Arc::ptr_eq(installed, &bridge));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(RuntimeFailure::Refused(errors.join("; ")))
    }
}

pub(super) fn begin_turn(context: &RuntimeContext) {
    if let Some(bridge) = BRIDGES
        .lock()
        .get(&context.tab_id)
        .filter(|bridge| bridge.epoch == context.epoch && bridge.nonce == context.binding_nonce)
    {
        let mut state = bridge.state.lock();
        state.generation += 1;
        state.closed = false;
    }
}

pub(super) async fn cancel_all(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    close: bool,
) -> Result<(), RuntimeFailure> {
    session
        .prompts_cancelled
        .store(true, std::sync::atomic::Ordering::Release);
    let bridges = cancel(svc, session, process, close).await;
    let permissions = super::prompt_answers::cancel_pending(svc, session, process).await;
    bridges.and(permissions)
}
