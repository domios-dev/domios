use crate::agent::provider::ModelInfo;
use crate::service::spawn::catalogue_lookup_model;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReplacementReason {
    SameFamilyLatest,
    ProviderRecommended,
    FirstAvailable,
}

impl ReplacementReason {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::SameFamilyLatest => "latest model in the same family",
            Self::ProviderRecommended => "the provider's recommended model",
            Self::FirstAvailable => "the first model the provider offers",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Replacement {
    pub model: String,
    pub reason: ReplacementReason,
}

pub(crate) struct CatalogueView<'a> {
    pub models: &'a [ModelInfo],
    pub recommended: Option<&'a str>,
    pub authoritative: bool,
    /// Free-form catalogues accept unlisted ids, so only a listed id that vanished is dead.
    pub previously_listed: &'a [String],
}

/// `None` keeps the stored default: it is empty, still offered, or not provably gone.
pub(crate) fn replacement_for(old: &str, catalogue: &CatalogueView<'_>) -> Option<Replacement> {
    let old = old.trim();
    if old.is_empty() || offered(catalogue.models, old) {
        return None;
    }
    let available: Vec<&ModelInfo> = catalogue
        .models
        .iter()
        .filter(|model| model.available != Some(false))
        .collect();
    let first = available.first()?;
    let was_listed = catalogue
        .previously_listed
        .iter()
        .any(|id| id.eq_ignore_ascii_case(old));
    if !catalogue.authoritative && !was_listed {
        return None;
    }
    if let Some(model) = latest_in_family(old, &available) {
        return Some(Replacement {
            model,
            reason: ReplacementReason::SameFamilyLatest,
        });
    }
    if let Some(model) = catalogue
        .recommended
        .and_then(|recommended| offered_model(&available, recommended))
    {
        return Some(Replacement {
            model,
            reason: ReplacementReason::ProviderRecommended,
        });
    }
    Some(Replacement {
        model: first.id.clone(),
        reason: ReplacementReason::FirstAvailable,
    })
}

pub(crate) fn offered(models: &[ModelInfo], requested: &str) -> bool {
    let available: Vec<&ModelInfo> = models
        .iter()
        .filter(|model| model.available != Some(false))
        .collect();
    offered_model(&available, requested).is_some()
}

fn offered_model(available: &[&ModelInfo], requested: &str) -> Option<String> {
    let owned: Vec<ModelInfo> = available.iter().map(|model| (*model).clone()).collect();
    let lookup = catalogue_lookup_model(requested, &owned);
    available
        .iter()
        .find(|model| {
            model.id.eq_ignore_ascii_case(&lookup)
                || model.display_name.eq_ignore_ascii_case(&lookup)
        })
        .map(|model| model.id.clone())
}

fn latest_in_family(old: &str, available: &[&ModelInfo]) -> Option<String> {
    let (family, _) = family_and_version(old);
    if family.is_empty() {
        return None;
    }
    let mut best: Option<(&ModelInfo, Vec<u64>)> = None;
    for &model in available {
        let (candidate_family, version) = family_and_version(&model.id);
        if candidate_family != family {
            continue;
        }
        if best.as_ref().is_none_or(|(_, current)| version > *current) {
            best = Some((model, version));
        }
    }
    best.map(|(model, _)| model.id.clone())
}

/// Numeric segments are the version; the rest names the family, so
/// `gpt-5.1-codex` is family `gpt-codex` at version `[5, 1]`.
pub(crate) fn family_and_version(id: &str) -> (String, Vec<u64>) {
    let lower = id.to_ascii_lowercase();
    let mut family = Vec::new();
    let mut version = Vec::new();
    for part in lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
    {
        match part.parse::<u64>() {
            Ok(number) => version.push(number),
            Err(_) => family.push(part),
        }
    }
    (family.join("-"), version)
}

#[cfg(test)]
mod tests;
