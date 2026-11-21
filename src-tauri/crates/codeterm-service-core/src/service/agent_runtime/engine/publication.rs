use super::*;

pub fn publish(svc: &ServiceContext, tab: &str, session: &Session) {
    publish_state(svc, tab, session, true);
}

/// Publishes a degrade no restart owns: tracked requests end and held sends get their outcome now.
pub(in crate::service::agent_runtime) fn publish_degraded(
    svc: &ServiceContext,
    tab: &str,
    session: &Arc<Session>,
) {
    publish(svc, tab, session);
    if !session.turn_requests.lock().is_empty() {
        let (owner, runtime) = (svc.clone(), session.clone());
        crate::async_runtime::spawn(async move {
            super::super::turn_requests::end_for_lifecycle(
                &owner,
                &runtime,
                "Runtime degraded before the turn completion reply",
            )
            .await;
        });
    }
    svc.wake_user_owner_on_state_change(tab);
}

pub(in crate::service::agent_runtime) fn publish_controls(
    svc: &ServiceContext,
    tab: &str,
    session: &Session,
) {
    publish_state(svc, tab, session, false);
}

fn publish_state(svc: &ServiceContext, tab: &str, session: &Session, lifecycle: bool) {
    let context = session.context.lock().clone();
    let account = session.account.lock().clone();
    let mut value = serde_json::to_value(snapshot(session)).unwrap_or(Value::Null);
    if let Some(snapshot) = value.as_object_mut() {
        super::super::declared_controls::project(session, snapshot);
        snapshot.insert(
            "loginBinding".into(),
            serde_json::json!({"nonce":context.binding_nonce,"epoch":context.epoch}),
        );
        snapshot.insert(
            "auth".into(),
            serde_json::to_value(session.auth.lock().clone()).unwrap_or(Value::Null),
        );
        snapshot.insert(
            "login".into(),
            serde_json::to_value(super::super::login::project(
                session.login.lock().login.as_ref(),
                super::super::login::now_ms(),
            ))
            .unwrap_or(Value::Null),
        );
        let (history, pages) = {
            let cache = session.cache.lock();
            (cache.backfill(), cache.history_pages())
        };
        snapshot.insert(
            "history".into(),
            serde_json::to_value(history).unwrap_or(Value::Null),
        );
        snapshot.insert("historyPages".into(), Value::from(pages));
        let adapter = session.adapter.lock().clone();
        if let Some(adapter) = adapter {
            if matches!(session.state.lock().phase, RuntimePhase::LoginPending)
                && !session
                    .process
                    .lock()
                    .as_ref()
                    .is_some_and(|process| process.ended.borrow().is_none())
            {
                let profile = svc.mux.read().get_tab(tab).and_then(|target| {
                    target
                        .agent_runtime_config
                        .as_ref()?
                        .get("launchProfile")?
                        .as_str()
                        .map(str::to_owned)
                });
                if let Some(profile) = adapter
                    .agent_runtime()
                    .and_then(|runtime| runtime.launch_profiles?.get(profile.as_deref()?).cloned())
                    .filter(|profile| !profile.secret_env.is_empty())
                {
                    snapshot.insert(
                        "requiredSecrets".into(),
                        serde_json::json!(profile
                            .secret_env
                            .values()
                            .collect::<std::collections::BTreeSet<_>>()),
                    );
                    snapshot.insert(
                        "loginProviderName".into(),
                        Value::String(adapter.display_name().into()),
                    );
                }
            }
            if let Some(login) = adapter.agent_runtime().and_then(|runtime| runtime.login) {
                snapshot.insert(
                    "loginMethods".into(),
                    serde_json::to_value(login.methods).unwrap_or(Value::Null),
                );
                snapshot.insert(
                    "loginProviderName".into(),
                    Value::String(adapter.display_name().into()),
                );
                snapshot.insert(
                    "loginTerminal".into(),
                    Value::Bool(login.terminal_command.is_some()),
                );
            }
            if let Some(methods) = super::super::auth_env::selected_profile(svc, tab, session)
                .map(|profile| super::super::auth_env::api_key_methods(&profile))
                .filter(|methods| !methods.is_empty())
            {
                snapshot.insert("apiKeyMethods".into(), serde_json::json!(methods));
            }
        }
    }
    #[cfg(any(test, feature = "test-support"))]
    publication_gate(tab);
    {
        let mut mux = svc.mux.write();
        let Some(target) = mux.get_tab_mut(tab) else {
            return;
        };
        if crate::service::provider_accounts::selected_account(target) != account
            || target.agent_type.as_deref() != Some(session.provider.as_str())
            || target.session_binding_nonce.as_deref() != Some(context.binding_nonce.as_str())
        {
            return;
        }
        if ["active_turn_started_at_ms", "last_turn_duration_ms"]
            .iter()
            .any(|field| {
                target
                    .agent_runtime_state
                    .as_ref()
                    .and_then(|saved| saved.get(*field))
                    != value.get(*field)
            })
        {
            target.record_activity();
        }
        if lifecycle
            && value["state"] == "loaded"
            && context.thread_id.is_some()
            && target.agent_session_id == context.thread_id
            && target.reported_session_binding_nonce.as_deref()
                == Some(context.binding_nonce.as_str())
            && session
                .thread_loaded
                .load(std::sync::atomic::Ordering::Acquire)
            && session
                .process
                .lock()
                .as_ref()
                .is_some_and(|process| process.ended.borrow().is_none())
        {
            crate::service::boot_stage::apply_boot_stage(
                target,
                codeterm_types::pty::BootStage::AgentReady,
                None,
            );
            if target
                .neighbor_authority
                .as_ref()
                .is_some_and(|authority| svc.neighbor_friend_paused(&authority.friendship_id))
            {
                target.frozen = true;
            }
        }
        if let Some(previous) = &target.agent_runtime_state {
            super::super::background::preserve_projection(previous, &mut value, &context);
        }
        target.agent_runtime_state = Some(value.clone());
        if lifecycle {
            target.agent_booting = value["state"] == "loading";
        }
    }
    if lifecycle && matches!(session.state.lock().phase, RuntimePhase::Degraded { .. }) {
        super::super::subscriber::release_lost_turn(svc, session);
    }
    session.control_changes.send_replace(());
    svc.notify_event(
        "agent-runtime-state",
        serde_json::json!({"tab_id":tab,"session_binding_nonce":context.binding_nonce,"state":value}),
    );
    crate::service::provider_accounts::login_changed(svc, tab);
}

#[cfg(any(test, feature = "test-support"))]
type PublicationGate = (std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>);
#[cfg(any(test, feature = "test-support"))]
static PUBLICATION_GATES: std::sync::LazyLock<
    parking_lot::Mutex<std::collections::HashMap<String, PublicationGate>>,
> = std::sync::LazyLock::new(Default::default);
#[cfg(any(test, feature = "test-support"))]
pub fn hold_publication_for_test(
    tab: &str,
) -> (std::sync::mpsc::Receiver<()>, std::sync::mpsc::Sender<()>) {
    let (entered, observed) = std::sync::mpsc::channel();
    let (release, resume) = std::sync::mpsc::channel();
    assert!(PUBLICATION_GATES
        .lock()
        .insert(tab.into(), (entered, resume))
        .is_none());
    (observed, release)
}
#[cfg(any(test, feature = "test-support"))]
fn publication_gate(tab: &str) {
    let gate = PUBLICATION_GATES.lock().remove(tab);
    if let Some((entered, resume)) = gate {
        entered.send(()).expect("publication observer remains live");
        resume
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("publication is released before the failure deadline");
    }
}
