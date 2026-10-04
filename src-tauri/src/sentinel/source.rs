//! Tab-scoped observation sources and the cron run a stage observes.

use serde::{Deserialize, Serialize};

use super::machine::Route;
use super::store::Sentinel;

/// What a graph stage observes instead of running a shell probe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum TabBinding {
    Tab(String),
    CronRun(String),
}

impl TabBinding {
    pub fn id(&self) -> &str {
        match self {
            Self::Tab(id) | Self::CronRun(id) => id,
        }
    }
}

/// The cron run whose spawned tab a `CronRun` stage observes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunBinding {
    pub run_id: String,
    pub tab_id: String,
    pub bound_at: u64,
}

pub(super) fn bounded_id(id: &str) -> bool {
    !id.trim().is_empty() && id.len() <= 256 && !id.chars().any(char::is_control)
}

/// A stage observes either its shell probe or a tab, only a cron-run source binds a run, and
/// prompt answers require a tab source.
pub(super) fn validate(job: &Sentinel) -> Result<(), String> {
    if let Some(binding) = &job.observe {
        if !job.probe.is_empty() || !bounded_id(binding.id()) {
            return Err("invalid sentinel observation source".into());
        }
    }
    if let Some(run) = &job.run {
        if !matches!(job.observe, Some(TabBinding::CronRun(_)))
            || !bounded_id(&run.run_id)
            || !bounded_id(&run.tab_id)
        {
            return Err("invalid sentinel run binding".into());
        }
    }
    let answers = job
        .routes
        .values()
        .flatten()
        .any(|handler| matches!(handler.action, Route::RespondPrompt(_)));
    if answers && job.observe.is_none() {
        return Err("respond_prompt handlers require a tab observation source".into());
    }
    Ok(())
}
