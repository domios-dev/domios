use std::fmt;

use crate::agent::provider::{ModelCatalogueAuthority, ModelInfo};
use crate::agent::registry::AgentRegistry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidModelError {
    pub provider_id: String,
    pub requested_model: String,
    pub valid_model_ids: Vec<String>,
    pub provider_mismatch: Option<String>,
    pub ambiguous_providers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentModelSelection {
    pub provider_id: String,
    pub model: Option<String>,
}

pub(crate) fn profile_scoped_options(registry: &AgentRegistry, provider: &str) -> bool {
    registry
        .with_provider(provider, |p| {
            p.agent_runtime()
                .is_some_and(|capability| capability.launch_profiles.is_some())
        })
        .unwrap_or(false)
}

impl InvalidModelError {
    pub fn rpc_data(&self) -> serde_json::Value {
        serde_json::json!({
            "type": if self.ambiguous_providers.is_empty() { "invalid_model" } else { "ambiguous_model" },
            "provider": self.provider_id.clone(),
            "requested_model": self.requested_model.clone(),
            "valid_model_ids": self.valid_model_ids.clone(),
            "ambiguous_providers": self.ambiguous_providers.clone(),
            "refresh_command": self.refresh_command(),
            "message": self.to_string(),
        })
    }

    pub(crate) fn refresh_command(&self) -> String {
        format!("domios agent models {}", self.provider_id)
    }
}

impl fmt::Display for InvalidModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.ambiguous_providers.is_empty() {
            return write!(
                formatter,
                "ambiguous model id '{}': owned by {}; pass --agent to select a provider on dispatch, or provide it positionally to agent spawn",
                self.requested_model,
                self.ambiguous_providers.join(", ")
            );
        }
        if let Some(requested_provider) = self.provider_mismatch.as_deref() {
            return write!(
                formatter,
                "model provider '{}' does not match selected provider '{}'; select that provider explicitly or choose one of its model ids",
                requested_provider, self.provider_id
            );
        }
        let suggestions = if self.valid_model_ids.is_empty() {
            "no model ids are currently advertised".to_string()
        } else {
            self.valid_model_ids.join(", ")
        };
        write!(
            formatter,
            "model id '{}' is not available from provider '{}'; closest valid model ids: {}",
            self.requested_model, self.provider_id, suggestions
        )
    }
}

pub fn model_catalogue_capability(
    registry: &AgentRegistry,
    provider_id: &str,
) -> crate::agent::provider::ModelCatalogueCapability {
    registry
        .with_provider(provider_id, |provider| {
            provider.model_catalogue_capability()
        })
        .unwrap_or_default()
}

pub(crate) fn normalize_requested_model(provider_id: &str, requested: &str) -> String {
    let requested = requested.trim();
    match requested.split_once(':') {
        Some((prefix, model)) if prefix.eq_ignore_ascii_case(provider_id) => model.trim().into(),
        _ => requested.into(),
    }
}

pub fn catalogue_lookup_model(requested: &str, catalogue: &[ModelInfo]) -> String {
    let requested = requested.trim();
    let without_context = requested
        .get(..requested.len().saturating_sub(4))
        .filter(|_| requested.to_ascii_lowercase().ends_with("[1m]"))
        .unwrap_or(requested);
    let lowercase = without_context.to_ascii_lowercase();
    let canonical = codeterm_types::agent::models::canonicalize_short(&lowercase);
    if canonical != lowercase
        && catalogue
            .iter()
            .any(|model| model.available != Some(false) && model.id.eq_ignore_ascii_case(canonical))
    {
        canonical.to_string()
    } else {
        lowercase
    }
}

/// The selectable entry a requested id names: by id or display name, after short-id canonicalization.
pub(crate) fn find_catalogue_model<'a>(
    requested: &str,
    catalogue: &'a [ModelInfo],
) -> Option<&'a ModelInfo> {
    let lookup = catalogue_lookup_model(requested, catalogue);
    catalogue.iter().find(|model| {
        model.available != Some(false)
            && (model.id.eq_ignore_ascii_case(&lookup)
                || model.display_name.eq_ignore_ascii_case(&lookup))
    })
}

/// The live snapshot, else the static list unless the provider requires live discovery.
fn launch_catalogue(registry: &AgentRegistry, provider_id: &str) -> Option<Vec<ModelInfo>> {
    if crate::agent::model_catalog::has_live_snapshot(provider_id) {
        crate::agent::model_catalog::resolve_live_models(provider_id)
    } else if model_catalogue_capability(registry, provider_id).live_discovery_required {
        None
    } else {
        Some(crate::agent::model_catalog::resolve_models(provider_id))
    }
}

/// The list a requested id must appear in; `None` leaves the id to the provider.
pub(crate) fn enforced_catalogue(
    registry: &AgentRegistry,
    provider_id: &str,
) -> Option<Vec<ModelInfo>> {
    let authoritative = model_catalogue_capability(registry, provider_id).authority
        == ModelCatalogueAuthority::Authoritative;
    if !authoritative && !crate::agent::model_catalog::has_live_snapshot(provider_id) {
        return None;
    }
    launch_catalogue(registry, provider_id)
}

pub fn catalogue_models_match(
    registry: &AgentRegistry,
    provider_id: &str,
    left: &str,
    right: &str,
) -> bool {
    let Some(catalogue) = launch_catalogue(registry, provider_id) else {
        return false;
    };
    let left = catalogue_model_identity(left, &catalogue);
    let right = catalogue_model_identity(right, &catalogue);
    matches!((left, right), (Some(left), Some(right)) if left.eq_ignore_ascii_case(&right))
}

fn catalogue_model_identity(requested: &str, catalogue: &[ModelInfo]) -> Option<String> {
    if let Some(model) = find_catalogue_model(requested, catalogue) {
        return Some(model.id.clone());
    }
    let lookup = catalogue_lookup_model(requested, catalogue);
    let requested_leaf = lookup.rsplit('/').next().unwrap_or(lookup.as_str());
    let matches = catalogue
        .iter()
        .filter(|model| model.available != Some(false))
        .filter(|model| {
            model
                .id
                .rsplit('/')
                .next()
                .is_some_and(|leaf| leaf.eq_ignore_ascii_case(requested_leaf))
        })
        .collect::<Vec<_>>();
    (matches.len() == 1).then(|| matches[0].id.clone())
}

pub(crate) fn preserve_context_suffix(resolved: String, requested: &str) -> String {
    if requested.to_ascii_lowercase().ends_with("[1m]") {
        format!("{resolved}[1m]")
    } else {
        resolved
    }
}

pub fn validate_agent_model(
    registry: &AgentRegistry,
    provider_id: &str,
    model: Option<&str>,
) -> Result<(), InvalidModelError> {
    let default_provider = provider_id.to_string();
    resolve_agent_model_selection(registry, Some(provider_id), model, &default_provider).map(|_| ())
}

pub fn resolve_agent_model_selection(
    registry: &AgentRegistry,
    explicit_provider: Option<&str>,
    model: Option<&str>,
    default_provider: &str,
) -> Result<AgentModelSelection, InvalidModelError> {
    let raw_requested = model.map(str::trim).filter(|value| !value.is_empty());
    if let Some(provider) =
        explicit_provider.filter(|provider| profile_scoped_options(registry, provider))
    {
        return Ok(AgentModelSelection {
            provider_id: provider.into(),
            model: raw_requested.map(str::to_owned),
        });
    }
    let providers = registry.provider_ids();
    let explicit_provider = explicit_provider
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let (provider_id, routed_model) = if let Some(explicit) = explicit_provider {
        if let Some(requested) = raw_requested {
            if let Some((qualified_provider, model_id)) =
                qualified_provider_model(requested, &providers)
            {
                if !qualified_provider.eq_ignore_ascii_case(explicit) {
                    return Err(provider_mismatch_error(
                        explicit,
                        requested,
                        &qualified_provider,
                    ));
                }
                (explicit.to_string(), Some(model_id))
            } else if providers
                .iter()
                .any(|provider| provider.eq_ignore_ascii_case(requested))
            {
                if !requested.eq_ignore_ascii_case(explicit) {
                    return Err(provider_mismatch_error(explicit, requested, requested));
                }
                (explicit.to_string(), None)
            } else {
                (explicit.to_string(), Some(requested.to_string()))
            }
        } else {
            (explicit.to_string(), None)
        }
    } else if let Some(requested) = raw_requested {
        if let Some((provider, model_id)) = qualified_provider_model(requested, &providers) {
            (provider, Some(model_id))
        } else if let Some(provider) = providers
            .iter()
            .find(|provider| provider.eq_ignore_ascii_case(requested))
        {
            (provider.clone(), None)
        } else {
            let owners = model_owners(registry, requested);
            match owners.as_slice() {
                [provider] => (provider.clone(), Some(requested.to_string())),
                [] => (default_provider.to_string(), Some(requested.to_string())),
                _ => {
                    return Err(InvalidModelError {
                        provider_id: default_provider.to_string(),
                        requested_model: requested.to_string(),
                        valid_model_ids: Vec::new(),
                        provider_mismatch: None,
                        ambiguous_providers: owners,
                    })
                }
            }
        }
    } else {
        (default_provider.to_string(), None)
    };
    if let Some(requested) = routed_model.as_deref() {
        let normalized = normalize_requested_model(&provider_id, requested);
        let selected_lists_model = provider_lists_model(registry, &provider_id, &normalized);
        validate_provider_model(registry, &provider_id, Some(&normalized))?;
        let foreign_owners = model_owners(registry, &normalized)
            .into_iter()
            .filter(|owner| !owner.eq_ignore_ascii_case(&provider_id))
            .collect::<Vec<_>>();
        if !selected_lists_model && !foreign_owners.is_empty() {
            return Err(provider_mismatch_error(
                &provider_id,
                &normalized,
                &foreign_owners.join(", "),
            ));
        }
    }
    Ok(AgentModelSelection {
        provider_id,
        model: routed_model,
    })
}

fn provider_mismatch_error(
    provider_id: &str,
    requested_model: &str,
    owner: &str,
) -> InvalidModelError {
    InvalidModelError {
        provider_id: provider_id.to_string(),
        requested_model: requested_model.to_string(),
        valid_model_ids: Vec::new(),
        provider_mismatch: Some(owner.to_string()),
        ambiguous_providers: Vec::new(),
    }
}

fn model_owners(registry: &AgentRegistry, model_id: &str) -> Vec<String> {
    let mut owners = registry
        .provider_ids()
        .into_iter()
        .filter(|provider_id| provider_lists_model(registry, provider_id, model_id))
        .collect::<Vec<_>>();
    owners.sort_unstable();
    owners
}

fn provider_lists_model(registry: &AgentRegistry, provider_id: &str, model_id: &str) -> bool {
    if provider_declares_model(registry, provider_id, model_id) {
        return true;
    }
    crate::agent::model_catalog::resolve_live_models(provider_id).is_some_and(|models| {
        models
            .iter()
            .any(|model| model.id.eq_ignore_ascii_case(model_id))
    })
}

fn provider_declares_model(registry: &AgentRegistry, provider_id: &str, model_id: &str) -> bool {
    registry
        .with_provider(provider_id, |provider| {
            provider.available_models().iter().any(|model| {
                model.available != Some(false) && model.id.eq_ignore_ascii_case(model_id)
            })
        })
        .unwrap_or(false)
}

fn qualified_provider_model(model: &str, provider_ids: &[String]) -> Option<(String, String)> {
    let (provider, model) = model.split_once(':')?;
    if provider_ids
        .iter()
        .any(|registered| registered.eq_ignore_ascii_case(provider))
        && !model.trim().is_empty()
    {
        Some((provider.to_string(), model.trim().to_string()))
    } else {
        None
    }
}

fn validate_provider_model(
    registry: &AgentRegistry,
    provider_id: &str,
    model: Option<&str>,
) -> Result<(), InvalidModelError> {
    let Some(raw_requested) = model.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    let requested = normalize_requested_model(provider_id, raw_requested);
    if let Some(endpoint) =
        crate::agent::model_catalog::resolve_endpoint_model(provider_id, &requested)
    {
        return endpoint
            .map(|_| ())
            .map_err(|valid_model_ids| InvalidModelError {
                provider_id: provider_id.to_string(),
                valid_model_ids: close_model_ids(&requested, &valid_model_ids),
                requested_model: requested,
                provider_mismatch: None,
                ambiguous_providers: Vec::new(),
            });
    }
    let capability = model_catalogue_capability(registry, provider_id);
    if capability.authority == ModelCatalogueAuthority::FreeForm
        && provider_declares_model(registry, provider_id, &requested)
    {
        return Ok(());
    }
    let Some(models) = enforced_catalogue(registry, provider_id) else {
        return Ok(());
    };
    if find_catalogue_model(&requested, &models).is_some() {
        return Ok(());
    }
    Err(InvalidModelError {
        provider_id: provider_id.to_string(),
        valid_model_ids: close_catalogue_ids(&requested, &models),
        requested_model: requested,
        provider_mismatch: None,
        ambiguous_providers: Vec::new(),
    })
}

/// How many close ids a refusal suggests; `agent models` lists the rest.
pub(crate) const CLOSE_ID_LIMIT: usize = 5;

/// Selectable catalogue ids ranked by closeness to a renamed or mistyped request.
pub(crate) fn close_catalogue_ids(requested: &str, catalogue: &[ModelInfo]) -> Vec<String> {
    let selectable = catalogue
        .iter()
        .filter(|model| model.available != Some(false))
        .map(|model| model.id.clone())
        .collect::<Vec<_>>();
    close_model_ids(requested, &selectable)
}

/// Closest first: edit distance, then longer shared prefix, then catalogue order.
pub(crate) fn close_model_ids(requested: &str, ids: &[String]) -> Vec<String> {
    let requested = requested.trim().to_ascii_lowercase();
    let mut unique: Vec<&String> = Vec::new();
    for id in ids {
        if !unique.iter().any(|seen| seen.eq_ignore_ascii_case(id)) {
            unique.push(id);
        }
    }
    let mut ranked = unique
        .into_iter()
        .enumerate()
        .map(|(order, id)| {
            let candidate = id.to_ascii_lowercase();
            let shared = requested
                .bytes()
                .zip(candidate.bytes())
                .take_while(|(left, right)| left == right)
                .count();
            let distance = edit_distance(&requested, &candidate);
            ((distance, std::cmp::Reverse(shared), order), id)
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| left.0.cmp(&right.0));
    ranked
        .into_iter()
        .take(CLOSE_ID_LIMIT)
        .map(|(_, id)| id.clone())
        .collect()
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    for (row, left_char) in left.chars().enumerate() {
        let mut current = vec![row + 1; right.len() + 1];
        for (column, right_char) in right.iter().enumerate() {
            let substitution = previous[column] + usize::from(left_char != *right_char);
            current[column + 1] = substitution
                .min(previous[column + 1] + 1)
                .min(current[column] + 1);
        }
        previous = current;
    }
    previous[right.len()]
}
