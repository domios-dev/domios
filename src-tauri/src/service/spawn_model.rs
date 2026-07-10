use crate::agent::registry::AgentRegistry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelFallbackNotice {
    pub requested: String,
    pub using: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FreshAgentModelResolution {
    pub model: Option<String>,
    pub fallback_notice: Option<ModelFallbackNotice>,
}

pub(crate) fn model_fallback_notice_title(notice: &ModelFallbackNotice) -> String {
    format!(
        "model {} unavailable -> using {}",
        notice.requested, notice.using
    )
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
        return FreshAgentModelResolution {
            model: requested.and_then(non_empty_model),
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
                requested: model,
                using: resolved.id.clone(),
            })
        } else {
            None
        };
        return FreshAgentModelResolution {
            model: non_empty_model(resolved.id),
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
        fallback_notice: None,
    }
}

pub(crate) fn non_empty_model(model: String) -> Option<String> {
    let trimmed = model.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}
