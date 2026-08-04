use crate::agent::registry::AgentRegistry;

pub use crate::multiplexer::types::ModelFallbackNotice;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FreshAgentModelResolution {
    pub requested_model: Option<String>,
    pub model: Option<String>,
    pub fallback_notice: Option<ModelFallbackNotice>,
}

pub(crate) fn attach_planned_model(
    resolution: &mut FreshAgentModelResolution,
    planned_model: Option<&str>,
) {
    let planned_model = planned_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string);
    if let Some(notice) = resolution.fallback_notice.as_mut() {
        notice.planned_model = planned_model;
    }
}

pub(crate) fn persist_model_launch_identity(
    tab: &mut crate::multiplexer::types::Tab,
    planned_model: Option<&str>,
    resolution: &FreshAgentModelResolution,
) {
    tab.agent_planned_model = planned_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string);
    tab.agent_requested_model = resolution.requested_model.clone();
    tab.agent_provider_resolved_model = resolution.model.clone();
    tab.agent_session_model = None;
    tab.model_fallback = resolution.fallback_notice.clone();
    tab.model_identity_mismatch = None;
}

pub(crate) fn model_fallback_notice_title(notice: &ModelFallbackNotice) -> String {
    format!(
        "model {} unavailable -> using {}",
        notice.requested_model, notice.provider_resolved_model
    )
}

/// [`codeterm_mem::worker_agent::BackgroundModelPort`] impl over the catalog
/// this crate owns — the seam that lets `codeterm-mem`'s shared worker-agent
/// resolver (used by every memory background-worker call site) reach
/// provider-model compatibility checking without a reverse crate dependency.
pub(crate) struct CatalogBackgroundModelPort;

impl codeterm_mem::worker_agent::BackgroundModelPort for CatalogBackgroundModelPort {
    fn resolve(&self, provider_id: &str, configured: &str) -> (String, bool) {
        let Some(configured) = non_empty_model(configured.to_string()) else {
            return (String::new(), false);
        };
        let canonical = codeterm_types::agent::models::canonicalize_short(&configured);
        let resolved = crate::agent::model_catalog::resolve_model(provider_id, Some(canonical));
        if resolved.fell_back {
            (String::new(), true)
        } else {
            (resolved.id, false)
        }
    }
}

pub(crate) fn resolve_fresh_agent_model(
    registry: &AgentRegistry,
    provider_id: &str,
    requested: Option<String>,
    resume_session_id: Option<&str>,
) -> Option<String> {
    resolve_fresh_agent_model_with_notice(registry, provider_id, requested, resume_session_id).model
}

pub(crate) fn resolve_fresh_agent_model_with_notice(
    registry: &AgentRegistry,
    provider_id: &str,
    requested: Option<String>,
    resume_session_id: Option<&str>,
) -> FreshAgentModelResolution {
    if resume_session_id.is_some() {
        let requested_model = requested.and_then(non_empty_model);
        return FreshAgentModelResolution {
            model: requested_model.clone(),
            requested_model,
            fallback_notice: None,
        };
    }
    if let Some(model) = requested.and_then(non_empty_model) {
        let resolved = crate::agent::model_catalog::resolve_model(provider_id, Some(&model));
        let fallback_notice = if resolved.fell_back {
            log::warn!(
                "[spawn] requested model '{}' unavailable for provider '{}'; using '{}'",
                model,
                provider_id,
                resolved.id
            );
            Some(ModelFallbackNotice {
                planned_model: None,
                requested_model: model.clone(),
                provider_resolved_model: resolved.id.clone(),
                actual_model: None,
                reason: "requested_model_unavailable".into(),
            })
        } else {
            None
        };
        return FreshAgentModelResolution {
            model: non_empty_model(resolved.id),
            requested_model: Some(model),
            fallback_notice,
        };
    }
    if let Some(model) = crate::config::provider_config(provider_id)
        .default_model
        .and_then(non_empty_model)
    {
        let resolved = crate::agent::model_catalog::resolve_model(provider_id, Some(&model));
        if !resolved.fell_back {
            return FreshAgentModelResolution {
                model: non_empty_model(resolved.id),
                requested_model: None,
                fallback_notice: None,
            };
        }
        log::warn!(
            "[spawn] ignoring stale default model '{}' for provider '{}'",
            model,
            provider_id
        );
    }
    let provider_default = registry
        .with_provider(provider_id, |p| p.default_model())
        .flatten()
        .and_then(non_empty_model);
    let resolved =
        crate::agent::model_catalog::resolve_model(provider_id, provider_default.as_deref());
    FreshAgentModelResolution {
        model: non_empty_model(resolved.id),
        requested_model: None,
        fallback_notice: None,
    }
}

pub(crate) fn non_empty_model(model: String) -> Option<String> {
    let trimmed = model.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}
