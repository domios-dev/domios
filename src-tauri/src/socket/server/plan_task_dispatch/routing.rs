use serde_json::json;

use crate::plan::Task;
use crate::service::SpawnIdentityProvenance;

use super::JsonRpcError;

pub(super) struct DispatchRouting {
    pub agent: Option<String>,
    pub model: Option<String>,
    pub(super) qualified_provider_mismatch: Option<(String, String)>,
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

pub(super) fn provider_mismatch_error(routing: &DispatchRouting) -> Option<JsonRpcError> {
    let Some((requested_provider, explicit_agent)) = &routing.qualified_provider_mismatch else {
        return None;
    };
    Some(JsonRpcError {
        code: -32602,
        message: format!(
            "model provider '{}' does not match explicitly selected provider '{}'",
            requested_provider, explicit_agent
        ),
        data: Some(json!({
            "type": "model_provider_mismatch",
            "requested_provider": requested_provider,
            "selected_provider": explicit_agent,
        })),
    })
}

pub(super) fn dispatch_worker_identity(
    task: &Task,
) -> (Option<String>, Option<SpawnIdentityProvenance>) {
    let title = crate::service::planned_task_title(task);
    let provenance = title
        .as_ref()
        .map(|_| SpawnIdentityProvenance::UserSupplied);
    (title, provenance)
}

/// Resolve the provider and model a dispatch should spawn. Precedence: an explicit
/// `agent` provider wins; a matching `<provider>:<model>` prefix is
/// normalized before provider resolution. Otherwise a requested qualified model
/// prefix or uniquely advertised bare model id selects its provider. A bare
/// provider name selects its default model; unavailable ids are refused later.
pub(super) fn resolve_dispatch_routing(
    explicit_agent: Option<String>,
    rpc_model: Option<String>,
    task_model: Option<String>,
    provider_ids: &[String],
) -> DispatchRouting {
    resolve_dispatch_routing_with_model_owners(
        explicit_agent,
        rpc_model,
        task_model,
        provider_ids,
        &[],
    )
}

pub(super) fn resolve_dispatch_routing_with_model_owners(
    explicit_agent: Option<String>,
    rpc_model: Option<String>,
    task_model: Option<String>,
    provider_ids: &[String],
    model_owners: &[(String, Vec<String>)],
) -> DispatchRouting {
    let clean = |s: Option<String>| s.filter(|v| !v.trim().is_empty());
    let requested_model = clean(rpc_model).or_else(|| clean(task_model));
    if let Some(agent) = clean(explicit_agent) {
        if let Some(model) = requested_model.as_deref() {
            if let Some((requested_provider, normalized_model)) =
                qualified_provider_model(model, provider_ids)
            {
                if requested_provider.eq_ignore_ascii_case(&agent) {
                    return DispatchRouting {
                        agent: Some(agent),
                        model: Some(normalized_model),
                        qualified_provider_mismatch: None,
                    };
                }
                return DispatchRouting {
                    agent: Some(agent.clone()),
                    model: requested_model,
                    qualified_provider_mismatch: Some((requested_provider, agent)),
                };
            }
        }
        return DispatchRouting {
            agent: Some(agent),
            model: requested_model,
            qualified_provider_mismatch: None,
        };
    }
    if let Some(model) = requested_model {
        if let Some((agent, model)) = qualified_provider_model(&model, provider_ids) {
            return DispatchRouting {
                agent: Some(agent),
                model: Some(model),
                qualified_provider_mismatch: None,
            };
        }
        if provider_ids.iter().any(|p| p.eq_ignore_ascii_case(&model)) {
            return DispatchRouting {
                agent: Some(model),
                model: None,
                qualified_provider_mismatch: None,
            };
        }
        let owners = model_owners
            .iter()
            .filter(|(_, models)| models.iter().any(|id| id.eq_ignore_ascii_case(&model)))
            .map(|(provider, _)| provider)
            .collect::<Vec<_>>();
        if owners.len() == 1 {
            return DispatchRouting {
                agent: Some(owners[0].clone()),
                model: Some(model),
                qualified_provider_mismatch: None,
            };
        }
        return DispatchRouting {
            agent: None,
            model: Some(model),
            qualified_provider_mismatch: None,
        };
    }
    DispatchRouting {
        agent: None,
        model: None,
        qualified_provider_mismatch: None,
    }
}
