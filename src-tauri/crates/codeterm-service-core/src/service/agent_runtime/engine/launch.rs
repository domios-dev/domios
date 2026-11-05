use super::{RuntimeFailure, RuntimeSettings, ServiceContext};

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
