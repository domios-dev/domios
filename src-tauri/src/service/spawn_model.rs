use super::spawn_model_catalogue::{
    catalogue_lookup_model, model_catalogue_capability, normalize_requested_model,
    preserve_context_suffix,
};
use super::ServiceContext;
use crate::agent::registry::AgentRegistry;
pub use crate::multiplexer::types::ModelFallbackNotice;

impl ServiceContext {
    pub(crate) fn reasoning_effort_launch_args_or_notice(
        &self,
        tab_id: &str,
        provider_id: &str,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> Vec<String> {
        let Some(effort) = effort.map(str::trim).filter(|effort| !effort.is_empty()) else {
            return Vec::new();
        };
        let result = self.agent_registry.with_provider(provider_id, |provider| {
            let supported = provider.reasoning_efforts_for_model(model);
            if supported.iter().any(|level| level.id == effort) {
                Ok(provider.reasoning_effort_launch_args(effort))
            } else {
                Err(supported
                    .into_iter()
                    .map(|level| level.id)
                    .collect::<Vec<_>>())
            }
        });
        match result {
            Some(Ok(args)) => args,
            Some(Err(supported)) => {
                self.notify_reasoning_effort_omitted(
                    tab_id,
                    provider_id,
                    model,
                    effort,
                    &supported,
                );
                Vec::new()
            }
            None => {
                self.notify_reasoning_effort_omitted(tab_id, provider_id, model, effort, &[]);
                Vec::new()
            }
        }
    }

    fn notify_reasoning_effort_omitted(
        &self,
        tab_id: &str,
        provider_id: &str,
        model: Option<&str>,
        effort: &str,
        supported: &[String],
    ) {
        let model = model.unwrap_or("provider default");
        let supported = if supported.is_empty() {
            "none".to_string()
        } else {
            supported.join(", ")
        };
        let title = format!("reasoning effort {effort} unavailable for model {model}");
        let body = format!(
            "The saved effort was omitted from the launch. Supported efforts: {supported}."
        );
        log::warn!(
            "[reasoning-effort] tab={tab_id} provider={provider_id} model={model} requested={effort} supported={supported}; omitted from launch"
        );
        self.publish_model_reconciliation_warning(
            title,
            Some(body),
            Some(tab_id),
        );
    }

    pub(crate) fn notify_model_fallback(&self, notice: &ModelFallbackNotice) {
        self.publish_model_reconciliation_warning(
            model_fallback_notice_title(notice),
            None,
            None,
        );
    }

    fn publish_model_reconciliation_warning(
        &self,
        title: String,
        body: Option<String>,
        tab_id: Option<&str>,
    ) {
        let origin = tab_id
            .map(|tab_id| {
                crate::service::notification::NotificationOrigin::local_tab(tab_id.to_string())
            })
            .unwrap_or_default();
        self.create_notification_ext(
            title,
            body,
            Some("warning"),
            None,
            None,
            origin,
            crate::service::notification::PushIntent::default(),
            None,
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FreshAgentModelResolution {
    pub requested_model: Option<String>,
    pub model: Option<String>,
    pub fallback_notice: Option<ModelFallbackNotice>,
    pub warning: Option<String>,
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
    tab.agent_session_model_identity = None;
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
        let requested_model = requested
            .and_then(non_empty_model)
            .map(|model| normalize_requested_model(provider_id, &model))
            .map(|model| normalize_session_model(&model));
        let warning = requested_model
            .as_deref()
            .filter(|_| live_catalogue_missing(registry, provider_id))
            .map(|model| unavailable_live_catalogue_warning(provider_id, model));
        return FreshAgentModelResolution {
            model: requested_model.clone(),
            requested_model,
            fallback_notice: None,
            warning,
        };
    }
    if let Some(model) = requested
        .and_then(non_empty_model)
        .map(|model| normalize_requested_model(provider_id, &model))
    {
        let catalogue_missing = live_catalogue_missing(registry, provider_id);
        let capability = model_catalogue_capability(registry, provider_id);
        let lookup = catalogue_lookup_model(&model);
        let matched = if catalogue_missing {
            None
        } else if capability.live_discovery_required {
            crate::agent::model_catalog::resolve_live_catalog_model(provider_id, &lookup)
        } else {
            crate::agent::model_catalog::resolve_catalog_model(provider_id, &lookup)
        };
        let resolved_model = matched
            .map(|entry| preserve_context_suffix(entry.id, &model))
            .unwrap_or_else(|| model.clone());
        let warning =
            catalogue_missing.then(|| unavailable_live_catalogue_warning(provider_id, &model));
        return FreshAgentModelResolution {
            model: non_empty_model(resolved_model).map(|id| normalize_session_model(&id)),
            requested_model: Some(normalize_session_model(&model)),
            fallback_notice: None,
            warning,
        };
    }
    if let Some(resolution) = resolve_authoritative_live_default(registry, provider_id) {
        return resolution;
    }
    if let Some(model) = crate::config::provider_config(provider_id)
        .default_model
        .and_then(non_empty_model)
    {
        let resolved = crate::agent::model_catalog::resolve_model(provider_id, Some(&model));
        if !resolved.fell_back {
            return FreshAgentModelResolution {
                model: non_empty_model(resolved.id).map(|id| normalize_session_model(&id)),
                requested_model: None,
                fallback_notice: None,
                warning: None,
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
        model: non_empty_model(resolved.id).map(|id| normalize_session_model(&id)),
        requested_model: None,
        fallback_notice: None,
        warning: None,
    }
}

fn live_catalogue_missing(registry: &AgentRegistry, provider_id: &str) -> bool {
    let capability = model_catalogue_capability(registry, provider_id);
    capability.authority == crate::agent::provider::ModelCatalogueAuthority::Authoritative
        && capability.live_discovery_required
        && !crate::agent::model_catalog::has_live_snapshot(provider_id)
}

fn resolve_live_provider_model(
    provider_id: &str,
    requested: &str,
) -> Option<crate::agent::provider::ModelInfo> {
    crate::agent::model_catalog::resolve_live_catalog_model(
        provider_id,
        &catalogue_lookup_model(requested),
    )
}

fn resolve_authoritative_live_default(
    registry: &AgentRegistry,
    provider_id: &str,
) -> Option<FreshAgentModelResolution> {
    let capability = model_catalogue_capability(registry, provider_id);
    if capability.authority != crate::agent::provider::ModelCatalogueAuthority::Authoritative
        || !capability.live_discovery_required
    {
        return None;
    }
    let configured_default = crate::config::provider_config(provider_id)
        .default_model
        .and_then(non_empty_model);
    let provider_default = registry
        .with_provider(provider_id, |provider| provider.default_model())
        .flatten()
        .and_then(non_empty_model);
    let Some(_models) = crate::agent::model_catalog::resolve_live_models(provider_id) else {
        let selected = configured_default.or(provider_default);
        let warning = match selected.as_deref() {
            Some(model) => format!(
                "provider '{provider_id}' requires a live model catalogue, but none is available; launching default model '{model}' unchanged. Run codeterm agent models {provider_id} to refresh and inspect the catalogue."
            ),
            None => format!(
                "provider '{provider_id}' requires a live model catalogue, but none is available and it declares no default model. Run codeterm agent models {provider_id} to refresh and inspect the catalogue."
            ),
        };
        return Some(FreshAgentModelResolution {
            model: selected.map(|model| normalize_session_model(&model)),
            requested_model: None,
            fallback_notice: None,
            warning: Some(warning),
        });
    };
    let configured_match = configured_default
        .as_deref()
        .and_then(|model| resolve_live_provider_model(provider_id, model));
    let fallback = provider_default
        .as_deref()
        .and_then(|requested| {
            resolve_live_provider_model(provider_id, requested)
                .map(|model| preserve_context_suffix(model.id, requested))
        })
        .or_else(|| crate::agent::model_catalog::resolve_live_default_model(provider_id));
    let selected = configured_match
        .map(|model| {
            preserve_context_suffix(model.id, configured_default.as_deref().unwrap_or_default())
        })
        .or_else(|| fallback.clone());
    let warning = if configured_default
        .as_deref()
        .is_some_and(|model| resolve_live_provider_model(provider_id, model).is_none())
    {
        Some(match fallback.as_deref() {
            Some(model) => format!(
                "configured default model '{}' is not in provider '{provider_id}' live catalogue; using live provider default '{model}'.",
                configured_default.as_deref().unwrap_or_default()
            ),
            None => format!(
                "configured default model '{}' is not in provider '{provider_id}' live catalogue, and no live default is available.",
                configured_default.as_deref().unwrap_or_default()
            ),
        })
    } else if configured_default.is_none()
        && provider_default
            .as_deref()
            .is_some_and(|model| resolve_live_provider_model(provider_id, model).is_none())
    {
        Some(match fallback.as_deref() {
            Some(model) => format!(
                "provider default model '{}' is not in provider '{provider_id}' live catalogue; using live provider default '{model}'.",
                provider_default.as_deref().unwrap_or_default()
            ),
            None => format!(
                "provider default model '{}' is not in provider '{provider_id}' live catalogue, and no live default is available.",
                provider_default.as_deref().unwrap_or_default()
            ),
        })
    } else {
        None
    };
    Some(FreshAgentModelResolution {
        model: selected.map(|model| normalize_session_model(&model)),
        requested_model: None,
        fallback_notice: None,
        warning,
    })
}

fn unavailable_live_catalogue_warning(provider_id: &str, model: &str) -> String {
    format!(
        "provider '{provider_id}' requires a live model catalogue, but none is available; launching requested model '{model}' unchanged. Run `codeterm agent models {provider_id}` to refresh and inspect the catalogue."
    )
}

/// The four short labels the catalog canonicalizes to. A model id outside this
/// set is the provider's own text and is kept verbatim.
const CANONICAL_SHORT_LABELS: [&str; 4] = ["opus", "sonnet", "haiku", "fable"];

/// One spelling for one model, whichever producer supplied it. The launch side
/// hands over catalog ids (`opus`, `claude-fable-5-1`) and the runtime detector
/// hands over the provider's display name (`Fable`); without this fold the two
/// disagree on the same tab and every label reader inherits the disagreement.
pub(crate) fn normalize_session_model(model: &str) -> String {
    let trimmed = model.trim();
    let context_suffix = trimmed.to_ascii_lowercase().ends_with("[1m]");
    let base = if context_suffix {
        trimmed
            .get(..trimmed.len().saturating_sub(4))
            .unwrap_or(trimmed)
    } else {
        trimmed
    };
    let lowered = base.to_ascii_lowercase();
    let canonical = codeterm_types::agent::models::canonicalize_short(&lowered);
    if CANONICAL_SHORT_LABELS.contains(&canonical) {
        if context_suffix {
            format!("{canonical}[1m]")
        } else {
            canonical.to_string()
        }
    } else {
        trimmed.to_string()
    }
}

/// A tab launched with a DECLARED model was dispatched as that agent; running a
/// different one is a failure, not a note. Without a declaration the operator
/// took the default and a substitution is only worth a warning.
pub(crate) fn unrequested_substitution_severity(planned_model: Option<&str>) -> &'static str {
    match planned_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        Some(_) => "error",
        None => "warning",
    }
}

pub(crate) fn non_empty_model(model: String) -> Option<String> {
    let trimmed = model.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod model_casing_tests {
    use super::*;

    #[test]
    fn both_producers_spell_a_known_model_the_same_way() {
        // The launch side supplies catalog ids; the runtime detector supplies
        // the provider's display name. Same tab, same label, either way.
        for (launch_side, runtime_side) in [
            ("opus", "Opus"),
            ("claude-opus-5", "OPUS"),
            ("fable", "Fable"),
            ("claude-fable-5-1", "Fable"),
            ("sonnet", " Sonnet "),
            ("haiku", "Haiku"),
        ] {
            assert_eq!(
                normalize_session_model(launch_side),
                normalize_session_model(runtime_side),
                "{launch_side} vs {runtime_side}"
            );
        }
    }

    #[test]
    fn a_known_model_folds_to_its_canonical_short_label() {
        assert_eq!(normalize_session_model("Fable"), "fable");
        assert_eq!(normalize_session_model("claude-opus-5"), "opus");
    }

    #[test]
    fn an_unknown_model_id_keeps_the_providers_own_text() {
        assert_eq!(normalize_session_model("GPT-5-Codex"), "GPT-5-Codex");
        assert_eq!(normalize_session_model("  qwen3-coder  "), "qwen3-coder");
    }

    #[test]
    fn an_unrequested_substitution_on_a_declared_model_is_a_failure() {
        assert_eq!(unrequested_substitution_severity(Some("opus")), "error");
    }

    #[test]
    fn a_substitution_with_no_declared_model_stays_a_warning() {
        for planned in [None, Some(""), Some("   ")] {
            assert_eq!(unrequested_substitution_severity(planned), "warning");
        }
    }
}
