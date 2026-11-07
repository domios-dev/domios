use super::{RuntimeFailure, RuntimeSettings, ServiceContext};

pub(super) fn refused_start(
    thread: Option<&str>,
    state: Option<&serde_json::Value>,
) -> Result<(), RuntimeFailure> {
    if let Some(state) = state.filter(|state| {
        state["state"] == "degraded"
            && (state["failureCode"] == "provider_refused"
                || (thread.is_none()
                    && super::super::start_failure::is_failed_unbound_state(state)))
    }) {
        return Err(RuntimeFailure::retained_refusal(
            state["reason"]
                .as_str()
                .unwrap_or("Runtime start was refused; start a new session"),
        ));
    }
    Ok(())
}

pub(super) async fn retire_refused_start(
    svc: &ServiceContext,
    session: &std::sync::Arc<super::Session>,
    reason: &RuntimeFailure,
) {
    if !matches!(reason, RuntimeFailure::Refused(_)) || session.context.lock().thread_id.is_some() {
        return;
    }
    let process = session.process.lock().take();
    if let Some(process) = process {
        if let Err(error) =
            super::super::client_bridge::cancel_all(svc, session, &process, true).await
        {
            log::debug!("Refused runtime bridge cleanup: {error}");
        }
        if let Err(error) = process.finish().await {
            log::debug!("Refused runtime process cleanup: {error}");
        }
    }
    *session.hook_state.lock() = serde_json::Value::Null;
    session.initialization.reset();
}

pub(super) fn require_launch_authority(
    svc: &ServiceContext,
    tab: &str,
) -> Result<(), RuntimeFailure> {
    let (provider, nonce) = svc
        .mux
        .read()
        .get_tab(tab)
        .and_then(|target| {
            Some((
                target.agent_type.clone()?,
                target.session_binding_nonce.clone()?,
            ))
        })
        .ok_or(RuntimeFailure::Superseded)?;
    if !super::super::authority::verifies(svc, tab, &provider, &nonce) {
        return Err(RuntimeFailure::Refused(
            "Runtime launch configuration is not committed".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_loaded_settings(
    requested: &RuntimeSettings,
    reported: &RuntimeSettings,
    model_matches: impl Fn(&str, &str) -> bool,
) -> Result<Option<String>, RuntimeFailure> {
    let mut notice = None;
    for (name, requested_value, reported_value) in [
        ("model", &requested.model, &reported.model),
        ("effort", &requested.effort, &reported.effort),
    ] {
        if name == "model"
            && reported.config_options.as_ref().is_some_and(|options| {
                options.iter().any(|option| {
                    option.category.as_deref() == Some("model")
                        && reported.model.as_deref() == Some(option.current_value.as_str())
                })
            })
        {
            if let (Some(requested), Some(reported)) = (requested_value, reported_value) {
                if requested != reported {
                    notice = Some(format!(
                        "Requested model {requested}, agent reports {reported}"
                    ));
                }
            }
            continue;
        }
        if let (Some(requested), Some(reported)) = (requested_value, reported_value) {
            let matches = if name == "model" {
                model_matches(requested, reported)
            } else {
                requested == reported
            };
            if !matches {
                return Err(RuntimeFailure::Refused(format!(
                    "Provider substituted {name}: requested {requested}, reported {reported}; no substitution reason supplied"
                )));
            }
        }
    }
    Ok(notice)
}

#[cfg(test)]
#[path = "launch/tests.rs"]
mod tests;
