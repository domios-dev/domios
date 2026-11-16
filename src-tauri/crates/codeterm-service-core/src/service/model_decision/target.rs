use std::collections::{BTreeMap, BTreeSet};

use codeterm_plan::service_plan::state::mission_control::ProviderAllowance;
use serde_json::Value;

use crate::agent::provider::{ModelCatalogueAuthority, ModelCatalogueCapability, ModelInfo};
use crate::service::spawn::spawn_model_catalogue::{enforced_catalogue, profile_scoped_options};
use crate::service::ServiceContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessReadiness {
    Ready,
    Absent,
    NotSignedIn,
    Quota,
}

/// One harness on the target, as `agent models` lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct HarnessCatalogue {
    pub readiness: HarnessReadiness,
    pub models: Vec<ModelInfo>,
    /// An id must appear in `models`; otherwise the provider decides unknown ids, as spawn does.
    pub enforced: bool,
    /// Provider-level effort ids in declared order, used when a model publishes none.
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
    pub default_model: Option<String>,
}

/// What one machine can launch. Unknown facts are never a veto.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TargetCatalogue {
    pub harnesses: BTreeMap<String, HarnessCatalogue>,
    /// `None` when the target reports no tool inventory.
    pub tools: Option<BTreeSet<String>>,
}

impl TargetCatalogue {
    pub fn readiness(&self, harness: &str) -> HarnessReadiness {
        self.harnesses
            .get(harness)
            .map_or(HarnessReadiness::Absent, |catalogue| catalogue.readiness)
    }

    /// This machine through the same catalogue seams spawn validation reads. Blocking.
    pub fn local(svc: &ServiceContext) -> Self {
        let registry = &svc.agent_registry;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| {
                i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
            });
        let allowances = crate::service::plan::mission_control_ext::provider_allowances(svc);
        let mut harnesses = BTreeMap::new();
        for id in registry.provider_ids() {
            let absent = svc
                .provider_registry
                .cli_status(&id, crate::agent::provider::current_platform_tag())
                .is_ok_and(|status| status.installed == Some(false));
            let exhausted = allowances
                .get(&id)
                .is_some_and(|allowance| allowance_exhausted(allowance, now_ms));
            let readiness = match (absent, exhausted) {
                (true, _) => HarnessReadiness::Absent,
                (false, true) => HarnessReadiness::Quota,
                (false, false) => HarnessReadiness::Ready,
            };
            let enforced = if profile_scoped_options(registry, &id) {
                None
            } else {
                enforced_catalogue(registry, &id)
            };
            let efforts: Vec<String> = registry
                .with_provider(&id, |provider| {
                    provider
                        .reasoning_efforts()
                        .into_iter()
                        .map(|level| level.id)
                        .collect()
                })
                .unwrap_or_default();
            let catalogue = HarnessCatalogue {
                readiness,
                enforced: enforced.is_some(),
                models: enforced
                    .unwrap_or_else(|| crate::agent::model_catalog::resolve_models(&id)),
                efforts,
                default_effort: crate::agent::model_catalog::resolve_default_reasoning_effort(&id),
                default_model: crate::agent::model_catalog::resolve_default(&id),
            };
            harnesses.insert(id, catalogue);
        }
        Self {
            harnesses,
            tools: None,
        }
    }

    /// A peer from its `/api/agents/providers` listing, the mapping `agent models --mesh` renders.
    pub fn from_provider_listing(providers: &[Value]) -> Self {
        let harnesses = providers
            .iter()
            .filter_map(|provider| {
                let id = provider.get("id")?.as_str()?.to_string();
                let cli = provider.get("cli_status").filter(|cli| !cli.is_null());
                let installed = cli
                    .and_then(|cli| cli.get("installed"))
                    .and_then(Value::as_bool);
                let profile_scoped = cli
                    .and_then(|cli| cli.get("launch_profiles"))
                    .is_some_and(|profiles| !profiles.is_null());
                let capability = provider
                    .get("model_catalogue")
                    .and_then(|value| {
                        serde_json::from_value::<ModelCatalogueCapability>(value.clone()).ok()
                    })
                    .unwrap_or_default();
                let models = provider
                    .get("available_models")
                    .and_then(|value| serde_json::from_value::<Vec<ModelInfo>>(value.clone()).ok())
                    .unwrap_or_default();
                let efforts: Vec<String> = provider
                    .get("reasoning_efforts")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|level| level.get("id")?.as_str().map(str::to_string))
                    .collect();
                let catalogue = HarnessCatalogue {
                    readiness: if installed == Some(false) {
                        HarnessReadiness::Absent
                    } else {
                        HarnessReadiness::Ready
                    },
                    models,
                    enforced: !profile_scoped
                        && (capability.authority == ModelCatalogueAuthority::Authoritative
                            || capability.live_discovery_required),
                    efforts,
                    default_effort: provider
                        .get("default_reasoning_effort")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    default_model: provider
                        .get("default_model")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                };
                Some((id, catalogue))
            })
            .collect();
        Self {
            harnesses,
            tools: None,
        }
    }
}

/// A fresh reading at or above the limit whose window has not reset yet.
pub(crate) fn allowance_exhausted(allowance: &ProviderAllowance, now_ms: i64) -> bool {
    allowance.stale_reason.is_none()
        && [
            (allowance.session_used_pct, allowance.session_resets_at),
            (allowance.weekly_used_pct, allowance.weekly_resets_at),
        ]
        .into_iter()
        .any(|(used, resets_at)| {
            used.is_some_and(|used| used >= 100.0)
                && resets_at.is_none_or(|resets_at| resets_at > now_ms)
        })
}
