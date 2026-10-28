use crate::service::ServiceContext;
use codeterm_plugin::remote_tasks::{RemoteTaskAvailability, RemoteTaskCapability};

pub(crate) fn resolve_mode(
    capability: Option<&RemoteTaskCapability>,
    mode: Option<&str>,
    environment: Option<&str>,
) -> Result<bool, String> {
    let Some(mode) = mode.filter(|mode| *mode != "local") else {
        return Ok(false);
    };
    if mode.trim().is_empty() {
        return Err("Execution mode must be explicit".into());
    }
    let capability = capability
        .filter(|capability| capability.mode == mode)
        .ok_or("The provider does not support this execution mode")?;
    if let RemoteTaskAvailability::Unavailable { reason } = &capability.availability {
        return Err(reason.clone());
    }
    if !capability
        .environments
        .iter()
        .any(|row| Some(row.id.as_str()) == environment)
    {
        return Err("Choose an available Cloud environment".into());
    }
    Ok(true)
}

pub(crate) fn restore_config(
    config: &mut serde_json::Value,
    mode: Option<&str>,
) -> Result<(), String> {
    let recorded = config
        .get("executionMode")
        .and_then(|value| value.as_str())
        .filter(|mode| *mode != "local");
    if recorded.is_some() && recorded != mode {
        return Err("Stored session configuration does not match this tab execution mode".into());
    }
    if let Some(mode) = mode {
        if config.is_null() {
            *config = serde_json::json!({});
        }
        config
            .as_object_mut()
            .ok_or("Invalid stored session configuration")?
            .insert("executionMode".into(), serde_json::json!(mode));
    }
    Ok(())
}

impl ServiceContext {
    pub(crate) fn persist_remote_launch(&self) -> Result<(), String> {
        match super::session_persist::write_session_snapshot(self, &self.mux)? {
            crate::session::store::SaveOutcome::Saved => Ok(()),
            crate::session::store::SaveOutcome::RefusedReduction(refusal) => Err(format!(
                "Desk persistence refused the Cloud tab: {refusal:?}"
            )),
        }
    }

    pub(crate) fn validate_remote_spawn(
        &self,
        params: &super::SpawnAgentParams,
    ) -> Result<bool, String> {
        let provider = params
            .agent
            .clone()
            .unwrap_or_else(|| self.configuration().general_agent.agent_provider);
        let cloud = self.remote_execution(
            &provider,
            &serde_json::json!({"executionMode": params.execution_mode, "preset": params.preset}),
        )?;
        if cloud
            && (params.role.is_some()
                || params.plan_task.is_some()
                || params.task_id.is_some()
                || params.model.is_some()
                || params.reasoning_effort.is_some()
                || params.speed.is_some()
                || params.system_prompt.is_some()
                || params.args.is_some()
                || params.resume_session_id.is_some()
                || params.fork_session
                || params.charter.is_some())
        {
            return Err("Cloud tasks do not support local roles, model controls, resume or launch arguments".into());
        }
        Ok(cloud)
    }
    pub(crate) fn remote_execution(
        &self,
        provider: &str,
        config: &serde_json::Value,
    ) -> Result<bool, String> {
        let capability = self
            .chatbackend_registry
            .descriptor(provider)
            .and_then(|descriptor| descriptor.remote_task)
            .or_else(|| {
                self.agent_registry
                    .with_provider(provider, |provider| provider.remote_task_capability())
                    .flatten()
            });
        if config
            .get("executionMode")
            .is_some_and(|mode| !mode.is_null() && !mode.is_string())
        {
            return Err("Execution mode must be a string".into());
        }
        resolve_mode(
            capability.as_ref(),
            config.get("executionMode").and_then(|mode| mode.as_str()),
            config.get("preset").and_then(|value| value.as_str()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_mode_requires_declared_available_environment_and_local_stays_local() {
        let capability: RemoteTaskCapability = serde_json::from_value(serde_json::json!({
            "mode":"fixture-cloud","displayName":"Fixture Cloud","backend":"fixture",
            "availability":{"status":"available"},"environments":[{"id":"env","label":"Fixture","accountRef":"account"}],
            "followUp":false,"cancel":false,"applyDiff":false
        })).unwrap();
        assert_eq!(resolve_mode(Some(&capability), None, None), Ok(false));
        assert_eq!(
            resolve_mode(Some(&capability), Some("local"), None),
            Ok(false)
        );
        assert_eq!(
            resolve_mode(Some(&capability), Some("fixture-cloud"), Some("env")),
            Ok(true)
        );
        assert!(resolve_mode(Some(&capability), Some("foreign"), Some("env")).is_err());
        assert!(resolve_mode(Some(&capability), Some("fixture-cloud"), None).is_err());
        assert!(resolve_mode(None, Some("fixture-cloud"), Some("env")).is_err());
    }
}

#[cfg(test)]
mod restore_tests {
    use super::*;
    #[test]
    fn restore_uses_only_exact_persisted_mode_and_never_promotes_local_history() {
        let mut absent = serde_json::Value::Null;
        restore_config(&mut absent, Some("opaque-cloud")).unwrap();
        assert_eq!(absent["executionMode"], "opaque-cloud");
        let mut local = serde_json::json!({});
        restore_config(&mut local, None).unwrap();
        assert!(local.get("executionMode").is_none());
        assert!(restore_config(
            &mut serde_json::json!({"executionMode":"foreign"}),
            Some("opaque-cloud")
        )
        .is_err());
        assert!(restore_config(
            &mut serde_json::json!({"executionMode":"opaque-cloud"}),
            None
        )
        .is_err());
    }
}
