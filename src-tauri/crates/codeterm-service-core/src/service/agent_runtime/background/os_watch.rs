use super::{
    refused, roster, snapshot, validate_current_session, BackgroundProcessAuthority,
    BackgroundProcessEntry, BackgroundProcessSnapshot, BackgroundStopDegradation,
    BackgroundStopResult,
};
use crate::service::{
    agent_runtime::{
        client_bridge,
        engine::{self, Session},
        model::RuntimeFailure,
        process::Process,
        process_boundary::{
            format_os_process_id, parse_os_process_id, wait_for_any_exit, ExitWaitCancel,
            HostMcpFilter, SelectedBackgroundProcess,
        },
    },
    ServiceContext,
};
use codeterm_plugin::{
    agent_runtime::{RuntimeBackgroundProcess, RuntimeContext, MAX_BACKGROUND_PROCESSES},
    api_contracts::BackgroundProcessChild,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

const BRIDGE_PREFIX: &str = "bridge:";

fn host_mcp_filter(context: &RuntimeContext) -> Option<HostMcpFilter> {
    context.host_mcp.as_ref().map(|mcp| HostMcpFilter {
        command: mcp.command.clone(),
        args: mcp.args.clone(),
    })
}

pub(super) fn reconcile_os_entries(
    root_thread_id: &str,
    active_turn: bool,
    observed: &[SelectedBackgroundProcess],
    bridge_terminals: &[(String, String, String, u32)],
    tracked: &mut HashMap<(u32, u128), Vec<(u32, u128)>>,
) -> (Vec<BackgroundProcessEntry>, HashSet<(u32, u128)>) {
    let previously_tracked: HashSet<(u32, u128)> = tracked
        .iter()
        .flat_map(|(root, members)| std::iter::once(*root).chain(members.iter().copied()))
        .collect();
    let mut kept_ids = HashSet::new();
    let mut next_tracked = HashMap::new();
    let mut entries = Vec::new();
    for item in observed {
        let key = (item.pid, item.born);
        let group_previously_tracked = previously_tracked.contains(&key)
            || item
                .children
                .iter()
                .any(|child| previously_tracked.contains(&(child.pid, child.born)));
        if active_turn && !group_previously_tracked {
            continue;
        }
        if entries.len() >= MAX_BACKGROUND_PROCESSES {
            break;
        }
        kept_ids.insert(key);
        let mut child_identities = Vec::with_capacity(item.children.len());
        let mut children = Vec::with_capacity(item.children.len());
        for child in &item.children {
            let child_key = (child.pid, child.born);
            kept_ids.insert(child_key);
            child_identities.push(child_key);
            children.push(BackgroundProcessChild {
                command: child.command.clone(),
                os_pid: Some(child.pid),
            });
        }
        next_tracked.insert(key, child_identities);
        entries.push(BackgroundProcessEntry {
            thread_id: root_thread_id.to_owned(),
            process: RuntimeBackgroundProcess {
                process_id: format_os_process_id(item.pid, item.born),
                item_id: format!("os-bg:{}:{}", item.pid, item.born),
                command: item.command.clone(),
                cwd: item.cwd.clone(),
                os_pid: Some(item.pid),
            },
            parent_os_pid: item.parent_pid,
            children: (!children.is_empty()).then_some(children),
        });
    }
    *tracked = next_tracked;
    for (id, command, cwd, pid) in bridge_terminals {
        if entries.len() >= MAX_BACKGROUND_PROCESSES {
            break;
        }
        let process_id = format!("{BRIDGE_PREFIX}{id}");
        if process_id.len() > 4096 || process_id.contains('\0') {
            continue;
        }
        let clean_cmd = command.replace('\0', "");
        let clean_cwd = cwd.replace('\0', "");
        entries.push(BackgroundProcessEntry {
            thread_id: root_thread_id.to_owned(),
            process: RuntimeBackgroundProcess {
                process_id: process_id.clone(),
                item_id: process_id,
                command: if clean_cmd.trim().is_empty() {
                    "Terminal".into()
                } else {
                    clean_cmd
                },
                cwd: if clean_cwd.trim().is_empty() || clean_cwd.len() > 4096 {
                    ".".into()
                } else {
                    clean_cwd
                },
                os_pid: Some(*pid),
            },
            parent_os_pid: None,
            children: None,
        });
    }
    (entries, kept_ids)
}

enum Wake {
    Stop,
    Refresh,
    WaitExited,
}

fn cancel_wait(
    wait_cancel: &mut Option<Arc<ExitWaitCancel>>,
    wait_handle: &mut Option<tokio::task::JoinHandle<()>>,
) {
    if let Some(cancel) = wait_cancel.take() {
        cancel.cancel();
    }
    *wait_handle = None;
}

pub(super) fn watch(svc: &ServiceContext, session: &Arc<Session>, process: &Arc<Process>) {
    let (svc, session, process) = (svc.clone(), session.clone(), process.clone());
    let epoch = session.context.lock().epoch;
    let mut ended = process.ended.clone();
    let mut pipe = process.client.ended();
    crate::async_runtime::spawn(async move {
        let mut wait_cancel: Option<Arc<ExitWaitCancel>> = None;
        let mut wait_handle: Option<tokio::task::JoinHandle<()>> = None;
        loop {
            if ended.borrow().is_some() || pipe.borrow().is_some() {
                break;
            }
            let wake = if let Some(handle) = wait_handle.as_mut() {
                tokio::select! {
                    biased;
                    _ = ended.changed() => Wake::Stop,
                    _ = pipe.changed() => Wake::Stop,
                    _ = process.background_changed.notified() => Wake::Refresh,
                    _ = handle => Wake::WaitExited,
                }
            } else {
                tokio::select! {
                    biased;
                    _ = ended.changed() => Wake::Stop,
                    _ = pipe.changed() => Wake::Stop,
                    _ = process.background_changed.notified() => Wake::Refresh,
                }
            };
            match wake {
                Wake::Stop => break,
                Wake::WaitExited => {
                    wait_cancel = None;
                    wait_handle = None;
                }
                Wake::Refresh => {
                    cancel_wait(&mut wait_cancel, &mut wait_handle);
                }
            }
            let context = session.context.lock().clone();
            if context.epoch != epoch || engine::validate_binding(&svc, &session).is_err() {
                break;
            }
            let Some(root_thread_id) = context.thread_id.clone() else {
                continue;
            };
            let authority = BackgroundProcessAuthority {
                root_thread_id,
                binding_nonce: context.binding_nonce.clone(),
                epoch,
            };
            match refresh_with_handles(&svc, &session, &process, &context, &authority).await {
                Ok((_, handles)) if !handles.is_empty() => match ExitWaitCancel::new() {
                    Ok(cancel) => {
                        let cancel = Arc::new(cancel);
                        let waiter_cancel = Arc::clone(&cancel);
                        let handle = tokio::task::spawn_blocking(move || {
                            wait_for_any_exit(handles, waiter_cancel);
                        });
                        wait_cancel = Some(cancel);
                        wait_handle = Some(handle);
                    }
                    Err(reason) => {
                        log::debug!(
                            "[background-work] tab={} exit waiter unavailable: {reason}",
                            context.tab_id
                        );
                    }
                },
                Ok(_) => {}
                Err(reason) => {
                    log::debug!(
                        "[background-work] tab={} os roster: {reason}",
                        context.tab_id
                    );
                }
            }
        }
        cancel_wait(&mut wait_cancel, &mut wait_handle);
        process.tracked_os_background.lock().clear();
        let context = session.context.lock().clone();
        if context.epoch == epoch {
            roster::publish(&svc, &context, None);
        }
    });
}

async fn refresh_with_handles(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    context: &RuntimeContext,
    authority: &BackgroundProcessAuthority,
) -> Result<
    (
        BackgroundProcessSnapshot,
        Vec<crate::service::agent_runtime::process_boundary::NativeMember>,
    ),
    RuntimeFailure,
> {
    let _operation = process.background_operation.lock().await;
    refresh_locked(svc, session, process, context, authority).await
}

async fn refresh_locked(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    context: &RuntimeContext,
    authority: &BackgroundProcessAuthority,
) -> Result<
    (
        BackgroundProcessSnapshot,
        Vec<crate::service::agent_runtime::process_boundary::NativeMember>,
    ),
    RuntimeFailure,
> {
    validate_current_session(svc, &context.tab_id, authority, session)?;
    let observed = process
        .observe_background(host_mcp_filter(context), context.cwd.clone())
        .await?;
    validate_current_session(svc, &context.tab_id, authority, session)?;
    let current = session.context.lock().clone();
    let active_turn = current.active_turn_id.is_some();
    let bridge_terminals = client_bridge::active_terminals(&current);
    let (entries, kept_ids) = {
        let mut tracked = process.tracked_os_background.lock();
        reconcile_os_entries(
            &authority.root_thread_id,
            active_turn,
            &observed.entries,
            &bridge_terminals,
            &mut tracked,
        )
    };
    let handles = observed
        .watch_handles
        .into_iter()
        .filter_map(|(pid, born, member)| kept_ids.contains(&(pid, born)).then_some(member))
        .collect();
    let snap = snapshot(authority, entries);
    roster::publish(svc, &current, Some(&snap));
    Ok((snap, handles))
}

pub(super) async fn list(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    context: &RuntimeContext,
    authority: &BackgroundProcessAuthority,
) -> Result<BackgroundProcessSnapshot, RuntimeFailure> {
    let (snap, _) = refresh_with_handles(svc, session, process, context, authority).await?;
    Ok(snap)
}

pub(super) async fn stop(
    svc: &ServiceContext,
    tab: &str,
    session: &Arc<Session>,
    process: &Arc<Process>,
    context: &RuntimeContext,
    authority: &BackgroundProcessAuthority,
    thread_id: &str,
    process_id: &str,
) -> Result<BackgroundStopResult, RuntimeFailure> {
    let _operation = process.background_operation.lock().await;
    validate_current_session(svc, tab, authority, session)?;
    if thread_id != authority.root_thread_id {
        return Err(RuntimeFailure::Superseded);
    }
    let cleanup = if let Some(terminal_id) = process_id.strip_prefix(BRIDGE_PREFIX) {
        client_bridge::stop_terminal(context, terminal_id)
            .await
            .map(|_| ())
            .map_err(|reason| {
                log::debug!("[background-work] tab={tab} bridge stop: {reason}");
                BackgroundStopDegradation::ExitUnconfirmed
            })
    } else if let Some((pid, born)) = parse_os_process_id(process_id) {
        let (targets, others) = {
            let tracked = process.tracked_os_background.lock();
            let mut targets = vec![(pid, born)];
            if let Some(children) = tracked.get(&(pid, born)) {
                targets.extend(children.iter().copied());
            }
            let target_set: HashSet<(u32, u128)> = targets.iter().copied().collect();
            let others: Vec<(u32, u128)> = tracked
                .iter()
                .flat_map(|(root, members)| std::iter::once(*root).chain(members.iter().copied()))
                .filter(|item| !target_set.contains(item))
                .collect();
            (targets, others)
        };
        let outcome = process.stop_os_background(targets, others).await;
        process.tracked_os_background.lock().remove(&(pid, born));
        outcome.map_err(|reason| {
            log::debug!("[background-work] tab={tab} os stop: {reason}");
            BackgroundStopDegradation::ExitUnconfirmed
        })
    } else {
        return Err(refused("Unknown background process id"));
    };
    let (snap, _) = refresh_locked(svc, session, process, context, authority).await?;
    process.background_changed.notify_one();
    Ok(match cleanup {
        Ok(()) => BackgroundStopResult::Stopped { snapshot: snap },
        Err(reason) => BackgroundStopResult::Degraded { reason },
    })
}

#[cfg(test)]
mod tests;
