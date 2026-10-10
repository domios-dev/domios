use codeterm_mem::worker_agent::resolve_worker_agent;
use codeterm_types::config::ReplacedModelDefault;

use super::replacement::Replacement;
use crate::config::schema::AppConfig;
use crate::cron::store::CronJob;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DefaultSlot {
    ProviderDefault(String),
    MemoryExtraction,
    MemoryDreaming,
    MemoryWorker,
    CronJob(String),
}

impl DefaultSlot {
    /// The setting path Settings reads the replacement note under.
    pub(crate) fn key(&self) -> String {
        match self {
            Self::ProviderDefault(provider) => format!("agents.{provider}.default_model"),
            Self::MemoryExtraction => "memory.extraction_model".into(),
            Self::MemoryDreaming => "memory.dreaming_model".into(),
            Self::MemoryWorker => "memory.worker_model".into(),
            Self::CronJob(id) => format!("cron.{id}.model"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredDefault {
    pub slot: DefaultSlot,
    pub provider: String,
    pub model: String,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlannedChange {
    pub stored: StoredDefault,
    pub replacement: Replacement,
}

/// Every model default the config stores, with the provider whose catalogue owns it.
pub(crate) fn config_defaults(config: &AppConfig) -> Vec<StoredDefault> {
    let mut defaults: Vec<StoredDefault> = config
        .agents
        .iter()
        .filter_map(|(provider, settings)| {
            let model = settings.default_model.as_deref()?.trim();
            (!model.is_empty()).then(|| StoredDefault {
                slot: DefaultSlot::ProviderDefault(provider.clone()),
                provider: provider.clone(),
                model: model.to_string(),
                subject: "Default model".into(),
            })
        })
        .collect();
    defaults.sort_by(|left, right| left.provider.cmp(&right.provider));
    let memory = &config.memory;
    let fallback_agent = config.general_agent.agent_provider.as_str();
    let extraction = resolve_worker_agent(None, &memory.extraction_agent, fallback_agent);
    let dreaming = resolve_worker_agent(None, &memory.dreaming_agent, fallback_agent);
    let worker_provider = if memory.extraction_model.trim().is_empty() {
        Some(extraction.clone())
    } else if memory.dreaming_model.trim().is_empty() {
        Some(dreaming.clone())
    } else {
        None
    };
    let mut push = |slot: DefaultSlot, provider: String, model: &str, subject: &str| {
        if !model.trim().is_empty() {
            defaults.push(StoredDefault {
                slot,
                provider,
                model: model.trim().to_string(),
                subject: subject.into(),
            });
        }
    };
    push(
        DefaultSlot::MemoryExtraction,
        extraction,
        &memory.extraction_model,
        "Memory extraction model",
    );
    push(
        DefaultSlot::MemoryDreaming,
        dreaming,
        &memory.dreaming_model,
        "Memory dreaming model",
    );
    if let Some(provider) = worker_provider {
        push(
            DefaultSlot::MemoryWorker,
            provider,
            &memory.worker_model,
            "Memory worker model",
        );
    }
    defaults
}

pub(crate) fn cron_defaults(jobs: &[CronJob]) -> Vec<StoredDefault> {
    jobs.iter()
        .filter_map(|job| {
            let model = job.model.as_deref()?.trim();
            (!model.is_empty()).then(|| StoredDefault {
                slot: DefaultSlot::CronJob(job.id.clone()),
                provider: job.agent.clone(),
                model: model.to_string(),
                subject: format!("Scheduled job \u{201c}{}\u{201d}", job.name),
            })
        })
        .collect()
}

/// Writes a planned change into the config and records the note Settings shows.
/// Cron slots are not config-owned and return false.
pub(crate) fn apply_to_config(config: &mut AppConfig, change: &PlannedChange) -> bool {
    let model = change.replacement.model.clone();
    match &change.stored.slot {
        DefaultSlot::ProviderDefault(provider) => {
            config
                .agents
                .entry(provider.clone())
                .or_default()
                .default_model = Some(model);
        }
        DefaultSlot::MemoryExtraction => config.memory.extraction_model = model,
        DefaultSlot::MemoryDreaming => config.memory.dreaming_model = model,
        DefaultSlot::MemoryWorker => config.memory.worker_model = model,
        DefaultSlot::CronJob(_) => return false,
    }
    record(config, change);
    true
}

pub(crate) fn record(config: &mut AppConfig, change: &PlannedChange) {
    config.model_defaults.replaced.insert(
        change.stored.slot.key(),
        ReplacedModelDefault {
            provider: change.stored.provider.clone(),
            from: change.stored.model.clone(),
            to: change.replacement.model.clone(),
            reason: change.replacement.reason.label().to_string(),
        },
    );
}

/// One notification body listing every default that moved.
pub(crate) fn notice_body(
    changes: &[PlannedChange],
    provider_name: impl Fn(&str) -> String,
) -> String {
    changes
        .iter()
        .map(|change| {
            format!(
                "{} for {}: {} is no longer offered, now {} ({}).",
                change.stored.subject,
                provider_name(&change.stored.provider),
                change.stored.model,
                change.replacement.model,
                change.replacement.reason.label(),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests;
