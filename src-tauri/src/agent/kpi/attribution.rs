//! Pure attribution + kind-mapping rules shared by every choke point. Kept free of
//! `ServiceContext` so the wiring contract is unit-testable without a live daemon.

use crate::agent::kpi_ledger::{KpiEventInput, KpiEventKind, TabCreatedKind, TokenCounts};
use crate::service::messenger::MessageKind;

/// Identity of the tab an event is charged to, resolved once per event at the choke point.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KpiAttribution {
    pub plan_id: Option<String>,
    pub task_id: Option<String>,
    pub tab_id: String,
    pub role: Option<&'static str>,
    pub provider: String,
    pub model: Option<String>,
    /// The workspace holding the tab when the event is written.
    pub workspace_id: Option<String>,
}

/// Descriptor labels keep the delivery path clear of `{:?}` formatting machinery.
fn role_label(
    role: crate::multiplexer::types::AgentRole,
    role_profile: Option<&str>,
) -> &'static str {
    codeterm_types::agent::role_kind::kind_label(Some(role), role_profile)
        .unwrap_or_else(|| role.label())
}

impl KpiAttribution {
    pub fn for_tab(tab_id: impl Into<String>) -> Self {
        Self {
            tab_id: tab_id.into(),
            ..Default::default()
        }
    }

    pub fn plan(mut self, plan_id: Option<String>, task_id: Option<String>) -> Self {
        self.plan_id = plan_id;
        self.task_id = task_id;
        self
    }

    pub fn role(
        mut self,
        role: Option<crate::multiplexer::types::AgentRole>,
        role_profile: Option<&str>,
    ) -> Self {
        self.role = role.map(|role| role_label(role, role_profile));
        self
    }

    pub fn provider(mut self, provider: Option<String>) -> Self {
        self.provider = provider.unwrap_or_default();
        self
    }

    pub fn model(mut self, model: Option<String>) -> Self {
        self.model = model;
        self
    }

    pub fn workspace(mut self, workspace_id: Option<String>) -> Self {
        self.workspace_id = workspace_id;
        self
    }

    /// Consumes the attribution — choke points resolve one per event, so moving the
    /// owned identity strings keeps the delivery path free of clone traffic.
    pub fn into_event(self, kind: KpiEventKind, bytes: u64) -> KpiEventInput {
        self.into_event_at(kind, bytes, now_epoch_secs())
    }

    pub fn into_event_at(self, kind: KpiEventKind, bytes: u64, timestamp: u64) -> KpiEventInput {
        KpiEventInput {
            plan_id: self.plan_id,
            task_id: self.task_id,
            tab_id: self.tab_id,
            role: self.role.map(str::to_string),
            provider: self.provider,
            model: self.model,
            timestamp,
            kind,
            bytes,
            tokens: None,
            dedup_key: None,
            workspace_id: self.workspace_id,
            tab_kind: None,
        }
    }

    pub fn event_at(&self, kind: KpiEventKind, bytes: u64, timestamp: u64) -> KpiEventInput {
        self.clone().into_event_at(kind, bytes, timestamp)
    }

    /// Same as [`Self::into_event`] but stamps a stable retry-safe `dedup_key` — use
    /// for kinds that have one (report id, `task_id:round:verdict`, ...).
    pub fn into_event_with_dedup(
        self,
        kind: KpiEventKind,
        bytes: u64,
        dedup_key: String,
    ) -> KpiEventInput {
        KpiEventInput {
            dedup_key: Some(dedup_key),
            ..self.into_event(kind, bytes)
        }
    }

    /// `response_key` names the provider response; the ledger keeps one row per key.
    pub fn token_event_at(
        &self,
        tokens: TokenCounts,
        timestamp: u64,
        response_key: String,
    ) -> KpiEventInput {
        KpiEventInput {
            tokens: Some(tokens),
            dedup_key: Some(response_key),
            ..self
                .clone()
                .into_event_at(KpiEventKind::TokenUsage, tokens.total(), timestamp)
        }
    }

    /// Creation precedes any task binding, so a created tab is charged to no plan.
    pub fn into_tab_created_event(self, kind: TabCreatedKind) -> KpiEventInput {
        KpiEventInput {
            plan_id: None,
            task_id: None,
            tab_kind: Some(kind),
            ..self.into_event(KpiEventKind::TabCreated, 0)
        }
    }
}

/// Which ledger kind a `Messenger::deliver` call contributes. `Reminder` returns `None`
/// because the nudge scheduler is that traffic's single writer — counting it here too
/// would double-charge the same bytes.
pub fn deliver_event_kind(kind: MessageKind) -> Option<KpiEventKind> {
    match kind {
        MessageKind::Message | MessageKind::Mesh | MessageKind::Telegram | MessageKind::Pebble => {
            Some(KpiEventKind::Send)
        }
        MessageKind::Report | MessageKind::Reminder | MessageKind::System => None,
    }
}

pub fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}
