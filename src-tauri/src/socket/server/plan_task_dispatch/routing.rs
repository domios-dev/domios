use serde_json::json;

use crate::plan::Task;
use crate::service::SpawnIdentityProvenance;

use super::JsonRpcError;

pub(super) struct DispatchRouting {
    pub agent: Option<String>,
    pub model: Option<String>,
    qualified_model: bool,
}

pub(super) fn qualified_provider_model(
    model: &str,
    provider_ids: &[String],
) -> Option<(String, String)> {
    let (provider, model) = model.split_once(':')?;
    if provider_ids
        .iter()
        .any(|registered| registered.eq_ignore_ascii_case(provider))
        && !model.trim().is_empty()
    {
        return Some((provider.to_string(), model.to_string()));
    }
    None
}

pub(super) fn unqualified_model_error(
    routing: &DispatchRouting,
    provider: &str,
    notice: &crate::service::spawn::ModelFallbackNotice,
) -> Option<JsonRpcError> {
    if routing.qualified_model {
        return None;
    }
    Some(JsonRpcError {
        code: -32602,
        message: format!(
            "model '{}' is unavailable for provider '{}'. Specify <provider>:<model> instead of silently substituting '{}'.",
            notice.requested_model, provider, notice.provider_resolved_model
        ),
        data: Some(json!({
            "type": "unqualified_model_unavailable",
            "requested_model": notice.requested_model,
            "default_provider": provider,
            "suggestion": "use <provider>:<model> or pass --agent <provider>",
        })),
    })
}

pub(super) fn dispatch_worker_identity(
    task: &Task,
) -> (Option<String>, Option<SpawnIdentityProvenance>) {
    let title = task
        .personality
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let provenance = if title.is_some() {
        match task.personality_provenance {
            Some(crate::plan::IdentityProvenance::UserSupplied) => {
                Some(SpawnIdentityProvenance::UserSupplied)
            }
            _ => Some(SpawnIdentityProvenance::Suggested),
        }
    } else {
        Some(SpawnIdentityProvenance::Suggested)
    };
    (title, provenance)
}

/// Resolve the provider and model a dispatch should spawn. Precedence: an explicit
/// `agent` provider wins (any model rides along verbatim); otherwise a requested
/// `<provider>:<model>` prefix is resolved against the live registry. A bare provider
/// name selects its default model; a bare model remains a request for the default
/// provider and is rejected if that provider cannot resolve it.
pub(super) fn resolve_dispatch_routing(
    explicit_agent: Option<String>,
    rpc_model: Option<String>,
    task_model: Option<String>,
    provider_ids: &[String],
) -> DispatchRouting {
    let clean = |s: Option<String>| s.filter(|v| !v.trim().is_empty());
    let requested_model = clean(rpc_model).or_else(|| clean(task_model));
    if let Some(agent) = clean(explicit_agent) {
        return DispatchRouting {
            agent: Some(agent),
            model: requested_model,
            qualified_model: false,
        };
    }
    if let Some(model) = requested_model {
        if let Some((agent, model)) = qualified_provider_model(&model, provider_ids) {
            return DispatchRouting {
                agent: Some(agent),
                model: Some(model),
                qualified_model: true,
            };
        }
        if provider_ids.iter().any(|p| p.eq_ignore_ascii_case(&model)) {
            return DispatchRouting {
                agent: Some(model),
                model: None,
                qualified_model: true,
            };
        }
        return DispatchRouting {
            agent: None,
            model: Some(model),
            qualified_model: false,
        };
    }
    DispatchRouting {
        agent: None,
        model: None,
        qualified_model: false,
    }
}
