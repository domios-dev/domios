//! After the Orchestrator is up, failures are never retried by the host: they reach the
//! coordinator once, as one routed message with a line per role or item.

use codeterm_team_package::contracts::{TemplateItemState, TemplateStartItem};

/// What happened to one role or payload item after the Orchestrator came up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PostStartStatus {
    pub role: String,
    pub outcome: Result<String, String>,
}

impl PostStartStatus {
    pub(crate) fn ok(role: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            outcome: Ok(detail.into()),
        }
    }

    pub(crate) fn failed(role: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            outcome: Err(reason.into()),
        }
    }
}

/// Registrations kept at another package version are reported, never overwritten.
pub(crate) fn pinned_statuses(registrations: &[TemplateStartItem]) -> Vec<PostStartStatus> {
    registrations
        .iter()
        .filter(|item| item.state == TemplateItemState::PinnedToOtherVersion)
        .map(|item| {
            PostStartStatus::failed(
                item.role.clone(),
                "already registered from another package version; kept as it was",
            )
        })
        .collect()
}

/// The one coordinator message, or nothing when every item succeeded.
pub(crate) fn summary_text(team_id: &str, statuses: &[PostStartStatus]) -> Option<String> {
    if statuses.iter().all(|status| status.outcome.is_ok()) {
        return None;
    }
    let mut text = format!(
        "Template start for Team {team_id} finished with problems. Nothing was retried; decide what to do:"
    );
    for status in statuses {
        let line = match &status.outcome {
            Ok(detail) => format!("\n- {}: ok, {detail}", status.role),
            Err(reason) => format!("\n- {}: failed, {reason}", status.role),
        };
        text.push_str(&line);
    }
    Some(text)
}
