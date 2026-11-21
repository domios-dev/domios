use super::super::{
    authority, background, capture, cli_exception, client_bridge, environment, login, transcript,
    turn_requests,
};
use super::*;

struct SwitchRetirement {
    source: crate::service::messenger::switch_handoff::SwitchSourceAuthority,
    context: RuntimeContext,
    process: Option<Arc<Process>>,
    cli: Mutex<Option<cli_exception::OwnedLease>>,
}

pub fn stop_switch_source_blocking(
    svc: &ServiceContext,
    source: &crate::service::messenger::switch_handoff::SwitchSourceAuthority,
) -> Result<(), String> {
    let expected = source.snapshot();
    let session =
        for_tab(&expected.id).ok_or("provider_switch_superseded: source runtime unavailable")?;
    let context = session.context.lock().clone();
    let process = session.process.lock().clone();
    {
        let mux = svc.mux.read();
        if !mux
            .get_tab(&expected.id)
            .is_some_and(|current| source.matches(current))
            || Some(session.provider.as_str()) != expected.agent_type.as_deref()
            || context.thread_id != expected.agent_session_id
            || Some(context.binding_nonce.as_str()) != expected.session_binding_nonce.as_deref()
        {
            return Err("provider_switch_superseded: source runtime authority changed".into());
        }
    }
    let retirement = SwitchRetirement {
        source: source.clone(),
        context,
        process,
        cli: Mutex::new(None),
    };
    let finish = || {
        crate::service::messenger::switch_handoff::with_switch_launch(svc, expected, || {
            let authority =
                crate::service::messenger::switch_handoff::launch_authority(svc, &expected.id);
            crate::async_runtime::block_on(crate::service::messenger::switch_handoff::scope_launch(
                authority,
                stop_session_scoped(svc, &expected.id, session, Some(&retirement)),
            ))
        })
    };
    let result = match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(finish)
        }
        Ok(_) => Err("Runtime stop requires a blocking executor".into()),
        Err(_) => finish(),
    };
    let cleanup = retirement
        .cli
        .lock()
        .take()
        .map(cli_exception::revoke_owned)
        .transpose()
        .map(|_| ());
    result.and(cleanup)
}

pub(super) async fn stop_session(
    svc: &ServiceContext,
    tab: &str,
    session: Arc<Session>,
) -> Result<(), String> {
    stop_session_scoped(svc, tab, session, None).await
}

async fn stop_session_scoped(
    svc: &ServiceContext,
    tab: &str,
    session: Arc<Session>,
    retirement: Option<&SwitchRetirement>,
) -> Result<(), String> {
    if !SESSIONS
        .lock()
        .get(tab)
        .is_some_and(|current| Arc::ptr_eq(current, &session))
    {
        return Err(RuntimeFailure::Superseded.to_string());
    }
    if retirement.is_none() {
        cancel_stop_retry(svc, tab);
    }
    if retirement.is_none() {
        turn_requests::end_for_lifecycle(
            svc,
            &session,
            "Runtime stopped before the turn completion reply",
        )
        .await;
    }
    let permit = session.gate.try_acquire().ok();
    if let Some(retirement) = retirement {
        let mux = svc.mux.read();
        let context = session.context.lock().clone();
        let current = session.process.lock().clone();
        if permit.is_none()
            || !mux
                .get_tab(tab)
                .is_some_and(|tab| retirement.source.matches_retired(tab))
            || context.epoch != retirement.context.epoch
            || context.binding_nonce != retirement.context.binding_nonce
            || context.thread_id != retirement.context.thread_id
            || !current
                .as_ref()
                .zip(retirement.process.as_ref())
                .is_some_and(|(current, expected)| Arc::ptr_eq(current, expected))
        {
            return Err("provider_switch_superseded: exact source runtime changed before process retirement".into());
        }
        session.gate.close();
    }
    if retirement.is_none() {
        session.gate.close();
    }
    if retirement.is_some() {
        turn_requests::end_for_lifecycle(
            svc,
            &session,
            "Runtime stopped before the turn completion reply",
        )
        .await;
    }
    let process = retirement
        .map(|retirement| retirement.process.clone())
        .unwrap_or_else(|| session.process.lock().clone());
    if retirement.is_none() && permit.is_some() {
        let should_close = session
            .state
            .lock()
            .session_capabilities
            .as_ref()
            .is_some_and(|caps| caps.close);
        let thread_id = session.context.lock().thread_id.clone();
        if let (true, Some(thread_id), Some(live_process)) = (should_close, thread_id, process.as_ref()) {
            let _ = tokio::time::timeout(
                Duration::from_secs(3),
                run(
                    svc,
                    &session.provider,
                    &session,
                    live_process,
                    RuntimeOperation::Control {
                        thread_id: Some(thread_id),
                        action: "closeSession".into(),
                        value: serde_json::json!({}),
                    },
                ),
            )
            .await;
        }
    }
    if let Some(process) = process {
        if let Err(reason) = client_bridge::cancel_all(svc, &session, &process, true).await {
            process.finish().await.map_err(|error| error.to_string())?;
            return Err(reason.to_string());
        }
        if let Err(reason) = background::cleanup(
            svc,
            &session,
            &process,
            background::Mode::Shutdown,
            Vec::new(),
            permit.is_some(),
        )
        .await
        {
            let epoch = session.context.lock().epoch;
            session.state.lock().degrade(epoch, &reason);
            publish(svc, tab, &session);
            return Err(reason.to_string());
        }
    }
    transcript::archive(&session).await;
    {
        let mux = retirement.map(|_| svc.mux.read());
        if retirement.is_some_and(|retirement| {
            !mux.as_ref()
                .and_then(|mux| mux.get_tab(tab))
                .is_some_and(|current| retirement.source.matches_retired(current))
        }) {
            return Err(
                "provider_switch_superseded: source context changed while runtime stopped".into(),
            );
        }
        let mut sessions = SESSIONS.lock();
        if !sessions
            .get(tab)
            .is_some_and(|current| Arc::ptr_eq(current, &session))
        {
            return Err(RuntimeFailure::Superseded.to_string());
        }
        if let Some(retirement) = retirement {
            if session.context.lock().epoch != retirement.context.epoch {
                return Err(
                    "provider_switch_superseded: source runtime epoch changed during retirement"
                        .into(),
                );
            }
            *retirement.cli.lock() =
                cli_exception::take_owned(&session.provider, &retirement.context)?;
            authority::revoke(tab);
            environment::revoke(tab);
            capture::revoke(tab);
        }
        sessions.remove(tab);
    }
    let cli_cleanup = if retirement.is_some() {
        Ok(())
    } else {
        authority::revoke(tab);
        environment::revoke(tab);
        capture::revoke(tab);
        let cli_tab = tab.to_string();
        codeterm_plugin::js_runtime::spawn_blocking(move || cli_exception::revoke(&cli_tab))
            .await
            .map_err(|error| error.to_string())
            .and_then(|result| result)
    };
    login::forget(&session);
    {
        let mut state = session.state.lock();
        state.failure_code = None;
        state.phase = RuntimePhase::Unloaded {
            reason: Some("Runtime stopped".into()),
        };
    }
    session.control_changes.send_replace(());
    session.process.close();
    cli_cleanup
}
