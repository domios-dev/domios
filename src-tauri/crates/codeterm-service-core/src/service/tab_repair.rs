use crate::service::MuxEmitCause;
use serde::Serialize;

use crate::agent::provider_identity::{provider_for_claimed_session, ProviderIdentityEvidence};
use crate::multiplexer::types::{Tab, TabBacking};
use crate::service::hierarchy::report_plan_ctx::tab_holds;
use crate::session::lifecycle::{
    checkpoint_history, AgentTypeWriteOrigin, JournalLoad, JournalRecord, LifecycleCheckpoint,
    LifecycleTransition, TailStatus,
};

use super::provider_identity::{AgentTypeWriteOutcome, TabAgentTypeState};
use super::{AuthenticatedPlanCaller, ServiceContext};

pub const TAB_REPAIR_VERIFIED_SESSION_REQUIRED: &str = "tab_repair_verified_session_required";
pub(crate) const TAB_REPAIR_NOT_AUTHORIZED: &str = "tab_repair_not_authorized";
pub(crate) const TAB_REPAIR_EVIDENCE_CHANGED: &str = "tab_repair_evidence_changed";
pub(crate) const TAB_REPAIR_LIVE_PTY_REQUIRED: &str = "tab_repair_live_pty_required";
pub(crate) const TAB_REPAIR_JOURNAL_UNAVAILABLE: &str = "tab_repair_journal_unavailable";
pub(crate) const TAB_REPAIR_JOURNAL_SOURCE_MISSING: &str = "tab_repair_journal_source_missing";
pub const TAB_REPAIR_SESSION_BINDING_PENDING: &str = "session_binding_pending";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabRepairErrorKind {
    NotFound,
    NotAuthorized,
    VerifiedSessionRequired,
    EvidenceChanged,
    LivePtyRequired,
    JournalUnavailable,
    JournalSourceMissing,
    SessionBindingPending,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabRepairError {
    pub kind: TabRepairErrorKind,
    pub message: String,
}

impl TabRepairError {
    pub fn new(kind: TabRepairErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        match self.kind {
            TabRepairErrorKind::NotFound => "tab_repair_target_not_found",
            TabRepairErrorKind::NotAuthorized => TAB_REPAIR_NOT_AUTHORIZED,
            TabRepairErrorKind::VerifiedSessionRequired => TAB_REPAIR_VERIFIED_SESSION_REQUIRED,
            TabRepairErrorKind::EvidenceChanged => TAB_REPAIR_EVIDENCE_CHANGED,
            TabRepairErrorKind::LivePtyRequired => TAB_REPAIR_LIVE_PTY_REQUIRED,
            TabRepairErrorKind::JournalUnavailable => TAB_REPAIR_JOURNAL_UNAVAILABLE,
            TabRepairErrorKind::JournalSourceMissing => TAB_REPAIR_JOURNAL_SOURCE_MISSING,
            TabRepairErrorKind::SessionBindingPending => TAB_REPAIR_SESSION_BINDING_PENDING,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuthorizedRepairTarget {
    pub tab_id: String,
    pub(crate) identity: ProviderIdentityEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TabRepairSource {
    ProviderAssertion,
    Session,
    LiveEvidence,
    Journal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RepairFieldStatus {
    Kept,
    Corrected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RepairField<T> {
    pub(crate) status: RepairFieldStatus,
    pub(crate) previous: Option<T>,
    pub(crate) current: Option<T>,
}

impl<T: PartialEq> RepairField<T> {
    fn from_values(previous: Option<T>, current: Option<T>) -> Self {
        let status = if previous == current {
            RepairFieldStatus::Kept
        } else {
            RepairFieldStatus::Corrected
        };
        Self {
            status,
            previous,
            current,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RepairEvidenceReport {
    pub(crate) session_id: String,
    pub(crate) provider: String,
    pub(crate) session_record_verified: bool,
    pub(crate) nonce_verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct JournalRecordReport {
    pub(crate) seq: u64,
    pub(crate) operation_id: String,
    pub(crate) record_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) recorded_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct SkippedJournalRecord {
    pub(crate) seq: u64,
    pub(crate) operation_id: String,
    pub(crate) record_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) recorded_at: Option<String>,
    pub(crate) reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JournalRepairReport {
    pub(crate) restored_from: JournalRecordReport,
    pub(crate) skipped: Vec<SkippedJournalRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TabRepairResponse {
    pub(crate) ok: bool,
    pub(crate) tab_id: String,
    pub(crate) source: TabRepairSource,
    pub(crate) provider: RepairField<String>,
    pub(crate) session: RepairField<String>,
    pub(crate) parser_lock: RepairField<bool>,
    pub(crate) evidence: RepairEvidenceReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) journal: Option<JournalRepairReport>,
}

impl TabRepairResponse {
    pub fn from_identity_write(
        tab_id: String,
        source: TabRepairSource,
        session_before: Option<String>,
        session_after: String,
        provider: String,
        nonce_verified: bool,
        outcome: AgentTypeWriteOutcome,
        journal: Option<JournalRepairReport>,
    ) -> Result<Self, TabRepairError> {
        let (previous, current) = identity_states(outcome)?;
        Ok(Self {
            ok: true,
            tab_id,
            source,
            provider: RepairField::from_values(previous.provider, current.provider),
            session: RepairField::from_values(session_before, Some(session_after.clone())),
            parser_lock: RepairField::from_values(Some(previous.locked), Some(current.locked)),
            evidence: RepairEvidenceReport {
                session_id: session_after,
                provider,
                session_record_verified: true,
                nonce_verified,
            },
            journal,
        })
    }
}

fn identity_states(
    outcome: AgentTypeWriteOutcome,
) -> Result<(TabAgentTypeState, TabAgentTypeState), TabRepairError> {
    match outcome {
        AgentTypeWriteOutcome::Applied { previous, current } => Ok((previous, current)),
        AgentTypeWriteOutcome::Kept { current } => Ok((current.clone(), current)),
        AgentTypeWriteOutcome::EvidenceChanged { .. } => Err(TabRepairError::new(
            TabRepairErrorKind::EvidenceChanged,
            "tab session evidence changed before the identity repair committed",
        )),
        AgentTypeWriteOutcome::RejectedClaimedSession { .. } => Err(TabRepairError::new(
            TabRepairErrorKind::VerifiedSessionRequired,
            "the identity boundary refused the requested provider for the claimed session",
        )),
    }
}

fn caller_matches_tab(tab: &Tab, caller: &AuthenticatedPlanCaller) -> bool {
    caller.tab_id.as_deref() == Some(tab.id.as_str())
        && tab.agent_session_id.is_some()
        && caller
            .session_id
            .as_deref()
            .is_none_or(|session_id| Some(session_id) == tab.agent_session_id.as_deref())
        && caller.binding_nonce.is_some()
        && caller.binding_nonce == tab.session_binding_nonce
        && caller.binding_nonce == tab.reported_session_binding_nonce
}

fn runtime_provider_for_repair(tab: &Tab) -> Option<String> {
    let TabBacking::ChatBackend {
        plugin_id,
        session_id,
    } = &tab.backing
    else {
        return None;
    };
    if tab.agent_session_id.as_deref() != Some(session_id.as_str())
        || !ProviderIdentityEvidence::capture(tab).nonce_verified()
    {
        return None;
    }
    let session = crate::service::agent_runtime::engine::for_tab(&tab.id)?;
    if session.provider != *plugin_id {
        return None;
    }
    let mut claimed = tab.clone();
    claimed.agent_type = Some(session.provider.clone());
    crate::service::agent_runtime::engine::has_live_bound_session(&claimed)
        .then(|| session.provider.clone())
}

fn live_task_plan(svc: &ServiceContext, tab: &Tab) -> Option<String> {
    let binding = tab.task_binding.as_ref()?;
    let plan_id = binding.plan_id.as_deref()?;
    let task_id = binding.task_id.as_deref()?;
    let plan = svc.plan_store.get(plan_id)?;
    let task = crate::plan::task_key::find_task(&plan.tasks, task_id)?;
    tab_holds(task, &tab.id).then(|| plan_id.to_string())
}

impl ServiceContext {
    #[cfg(test)]
    pub(crate) fn authorize_tab_repair(
        &self,
        tab_id: &str,
        caller: &AuthenticatedPlanCaller,
    ) -> Result<AuthorizedRepairTarget, TabRepairError> {
        let tab = self.mux.read().get_tab(tab_id).cloned().ok_or_else(|| {
            TabRepairError::new(
                TabRepairErrorKind::NotFound,
                format!("Tab {tab_id} was not found"),
            )
        })?;
        if !(matches!(&tab.backing, TabBacking::Pty) && tab.pty_id.is_some())
            && runtime_provider_for_repair(&tab).is_none()
        {
            return Err(TabRepairError::new(
                TabRepairErrorKind::LivePtyRequired,
                format!("Tab {tab_id} is not a live PTY tab"),
            ));
        }
        if caller_matches_tab(&tab, caller) {
            return Ok(AuthorizedRepairTarget {
                identity: ProviderIdentityEvidence::capture(&tab),
                tab_id: tab.id,
            });
        }
        let Some(plan_id) = live_task_plan(self, &tab) else {
            return Err(not_authorized(tab_id));
        };
        self.authorize_plan_orchestrator(&plan_id, caller)
            .map_err(|_| not_authorized(tab_id))?;
        Ok(AuthorizedRepairTarget {
            identity: ProviderIdentityEvidence::capture(&tab),
            tab_id: tab.id,
        })
    }

    pub async fn authorize_tab_repair_async(
        &self,
        tab_id: &str,
        caller: &AuthenticatedPlanCaller,
    ) -> Result<AuthorizedRepairTarget, TabRepairError> {
        let tab = self.mux.read().get_tab(tab_id).cloned().ok_or_else(|| {
            TabRepairError::new(
                TabRepairErrorKind::NotFound,
                format!("Tab {tab_id} was not found"),
            )
        })?;
        if !(matches!(&tab.backing, TabBacking::Pty) && tab.pty_id.is_some())
            && runtime_provider_for_repair(&tab).is_none()
        {
            return Err(TabRepairError::new(
                TabRepairErrorKind::LivePtyRequired,
                format!("Tab {tab_id} is not a live PTY tab"),
            ));
        }
        if caller_matches_tab(&tab, caller) {
            return Ok(AuthorizedRepairTarget {
                identity: ProviderIdentityEvidence::capture(&tab),
                tab_id: tab.id,
            });
        }
        let Some(plan_id) = live_task_plan(self, &tab) else {
            return Err(not_authorized(tab_id));
        };
        self.authorize_plan_orchestrator_async(&plan_id, caller)
            .await
            .map_err(|error| match error {
                super::PlanAuthorityError::SessionBindingPending => TabRepairError::new(
                    TabRepairErrorKind::SessionBindingPending,
                    error.to_string(),
                ),
                _ => not_authorized(tab_id),
            })?;
        Ok(AuthorizedRepairTarget {
            identity: ProviderIdentityEvidence::capture(&tab),
            tab_id: tab.id,
        })
    }

    pub async fn repair_tab_from_live_evidence(
        &self,
        target: AuthorizedRepairTarget,
        caller: &AuthenticatedPlanCaller,
        asserted_provider: Option<&str>,
    ) -> Result<TabRepairResponse, TabRepairError> {
        let source = if asserted_provider.is_some() {
            TabRepairSource::ProviderAssertion
        } else {
            TabRepairSource::LiveEvidence
        };
        let (session_id, provider) = self
            .verified_provider_for_repair(&target.tab_id, &target.identity)
            .await?;
        if asserted_provider.is_some_and(|asserted| asserted != provider.as_str()) {
            return Err(TabRepairError::new(
                TabRepairErrorKind::VerifiedSessionRequired,
                format!(
                    "bound session '{session_id}' verifies as provider '{provider}', not '{}'",
                    asserted_provider.unwrap_or_default()
                ),
            ));
        }
        let fresh = self
            .authorize_tab_repair_async(&target.tab_id, caller)
            .await?;
        if fresh.identity != target.identity {
            return Err(TabRepairError::new(
                TabRepairErrorKind::EvidenceChanged,
                "tab session evidence changed while provider evidence was inspected",
            ));
        }
        let outcome = self
            .write_verified_tab_agent_type_with_origin(
                &target.tab_id,
                provider.clone(),
                AgentTypeWriteOrigin::Repair,
                caller.tab_id.as_deref(),
                &target.identity,
            )
            .ok_or_else(|| {
                TabRepairError::new(
                    TabRepairErrorKind::NotFound,
                    format!("Tab {} was not found", target.tab_id),
                )
            })?;
        let response = TabRepairResponse::from_identity_write(
            target.tab_id,
            source,
            Some(session_id.clone()),
            session_id,
            provider,
            true,
            outcome,
            None,
        )?;
        {
            let mut mux = self.mux.write();
            if let Some(tab) = mux.get_tab_mut(&response.tab_id) {
                if runtime_provider_for_repair(tab).is_some() {
                    crate::service::boot_stage::apply_boot_stage(
                        tab,
                        codeterm_types::pty::BootStage::AgentReady,
                        None,
                    );
                }
            }
        }
        self.emit_state_changed(MuxEmitCause::AgentIdentity);
        Ok(response)
    }

    pub async fn repair_tab_from_journal(
        &self,
        target: AuthorizedRepairTarget,
        caller: &AuthenticatedPlanCaller,
    ) -> Result<TabRepairResponse, TabRepairError> {
        let (session_id, provider) = self
            .verified_provider_for_repair(&target.tab_id, &target.identity)
            .await?;
        let inspected_tab = target.tab_id.clone();
        let inspected_session = session_id.clone();
        let inspected_provider = provider.clone();
        let journal = self.lifecycle_journal.clone();
        let plan = tokio::task::spawn_blocking(move || {
            let load = journal.load();
            select_journal_restore(
                &load,
                &inspected_tab,
                &inspected_session,
                &inspected_provider,
            )
        })
        .await
        .map_err(|error| {
            TabRepairError::new(
                TabRepairErrorKind::JournalUnavailable,
                format!("lifecycle journal inspection failed: {error}"),
            )
        })??;
        let fresh = self
            .authorize_tab_repair_async(&target.tab_id, caller)
            .await?;
        if fresh.identity != target.identity {
            return Err(TabRepairError::new(
                TabRepairErrorKind::EvidenceChanged,
                "tab session evidence changed while the lifecycle journal was inspected",
            ));
        }
        let outcome = self
            .write_verified_tab_agent_type_with_origin(
                &target.tab_id,
                plan.provider.clone(),
                AgentTypeWriteOrigin::Repair,
                caller.tab_id.as_deref(),
                &target.identity,
            )
            .ok_or_else(|| {
                TabRepairError::new(
                    TabRepairErrorKind::NotFound,
                    format!("Tab {} was not found", target.tab_id),
                )
            })?;
        let response = TabRepairResponse::from_identity_write(
            target.tab_id,
            TabRepairSource::Journal,
            Some(session_id.clone()),
            session_id,
            provider,
            true,
            outcome,
            Some(plan.report),
        )?;
        {
            let mut mux = self.mux.write();
            if let Some(tab) = mux.get_tab_mut(&response.tab_id) {
                if runtime_provider_for_repair(tab).is_some() {
                    crate::service::boot_stage::apply_boot_stage(
                        tab,
                        codeterm_types::pty::BootStage::AgentReady,
                        None,
                    );
                }
            }
        }
        self.emit_state_changed(MuxEmitCause::AgentIdentity);
        Ok(response)
    }

    pub(crate) async fn verified_provider_for_repair(
        &self,
        tab_id: &str,
        evidence: &ProviderIdentityEvidence,
    ) -> Result<(String, String), TabRepairError> {
        if let Some(tab) = self.mux.read().get_tab(tab_id).cloned() {
            if ProviderIdentityEvidence::capture(&tab) == *evidence {
                if let Some(provider) = runtime_provider_for_repair(&tab) {
                    return Ok((tab.agent_session_id.unwrap_or_default(), provider));
                }
            }
        }
        let (session_id, cwd) = verified_session_query(evidence)?;
        let registry = self.agent_registry.clone();
        let inspected_session = session_id.clone();
        let provider = tokio::task::spawn_blocking(move || {
            provider_for_claimed_session(&registry, &cwd, &inspected_session)
        })
        .await
        .map_err(|error| {
            TabRepairError::new(
                TabRepairErrorKind::VerifiedSessionRequired,
                format!("provider session verification failed: {error}"),
            )
        })?
        .ok_or_else(|| {
            TabRepairError::new(
                TabRepairErrorKind::VerifiedSessionRequired,
                format!("session '{session_id}' does not have exactly one provider session record"),
            )
        })?;
        Ok((session_id, provider))
    }
}

fn verified_session_query(
    evidence: &ProviderIdentityEvidence,
) -> Result<(String, String), TabRepairError> {
    if !evidence.nonce_verified() {
        return Err(TabRepairError::new(
            TabRepairErrorKind::VerifiedSessionRequired,
            "the bound session has no matching configured and reported binding nonce",
        ));
    }
    let session_id = evidence.session_id.clone().ok_or_else(|| {
        TabRepairError::new(
            TabRepairErrorKind::VerifiedSessionRequired,
            "the tab has no bound provider session",
        )
    })?;
    let cwd = evidence.cwd.clone().ok_or_else(|| {
        TabRepairError::new(
            TabRepairErrorKind::VerifiedSessionRequired,
            "the tab has no working directory for provider session verification",
        )
    })?;
    Ok((session_id, cwd))
}

struct JournalRestorePlan {
    provider: String,
    report: JournalRepairReport,
}

fn select_journal_restore(
    load: &JournalLoad,
    tab_id: &str,
    verified_session_id: &str,
    verified_provider: &str,
) -> Result<JournalRestorePlan, TabRepairError> {
    if let TailStatus::Refused { line, reason } = &load.tail {
        return Err(TabRepairError::new(
            TabRepairErrorKind::JournalUnavailable,
            format!("lifecycle journal tail is unreadable at line {line}: {reason}"),
        ));
    }
    let mut skipped = Vec::new();
    let mut selected = None;
    for checkpoint in checkpoint_history(load, tab_id) {
        match checkpoint_skip_reason(&checkpoint, verified_session_id, verified_provider) {
            Some(reason) => skipped.push(skipped_checkpoint(&checkpoint, reason)),
            None => {
                selected = Some(checkpoint);
                break;
            }
        }
    }
    let selected = selected.ok_or_else(|| {
        TabRepairError::new(
            TabRepairErrorKind::JournalSourceMissing,
            format!(
                "no checkpoint for Tab {tab_id} matches nonce-verified session '{verified_session_id}' and provider '{verified_provider}'"
            ),
        )
    })?;
    skipped.extend(skipped_diagnostic_writes_after(
        load,
        tab_id,
        selected.committed_through,
    ));
    skipped.sort_by(|left, right| right.seq.cmp(&left.seq));
    Ok(JournalRestorePlan {
        provider: selected.authority.provider.clone(),
        report: JournalRepairReport {
            restored_from: checkpoint_report(&selected),
            skipped,
        },
    })
}

fn checkpoint_skip_reason(
    checkpoint: &LifecycleCheckpoint,
    verified_session_id: &str,
    verified_provider: &str,
) -> Option<String> {
    if checkpoint.authority.session_id != verified_session_id {
        return Some(format!(
            "checkpoint session '{}' does not match verified live session '{}'",
            checkpoint.authority.session_id, verified_session_id
        ));
    }
    if checkpoint.authority.provider != verified_provider {
        return Some(format!(
            "checkpoint provider '{}' does not own verified session '{}' (owner is '{}')",
            checkpoint.authority.provider, verified_session_id, verified_provider
        ));
    }
    let Some(write) = checkpoint.agent_type_write.as_ref() else {
        return None;
    };
    if write.origin == AgentTypeWriteOrigin::Parser {
        return Some("parser-origin checkpoint is never a restore source".into());
    }
    if !write.applied {
        return Some("checkpoint carries a refused identity write".into());
    }
    if !write.nonce_verified {
        return Some("checkpoint identity write was not nonce-verified".into());
    }
    if write.session_id.as_deref() != Some(verified_session_id) {
        return Some("checkpoint identity write names a different session".into());
    }
    if write.new_provider.as_deref() != Some(verified_provider) {
        return Some("checkpoint identity write disagrees with its session provider".into());
    }
    None
}

fn checkpoint_report(checkpoint: &LifecycleCheckpoint) -> JournalRecordReport {
    JournalRecordReport {
        seq: checkpoint.committed_through,
        operation_id: checkpoint.key.operation_id.clone(),
        record_type: "checkpoint".into(),
        recorded_at: checkpoint_recorded_at(checkpoint),
    }
}

fn skipped_checkpoint(checkpoint: &LifecycleCheckpoint, reason: String) -> SkippedJournalRecord {
    SkippedJournalRecord {
        seq: checkpoint.committed_through,
        operation_id: checkpoint.key.operation_id.clone(),
        record_type: "checkpoint".into(),
        recorded_at: checkpoint_recorded_at(checkpoint),
        reason,
    }
}

fn checkpoint_recorded_at(checkpoint: &LifecycleCheckpoint) -> Option<String> {
    checkpoint
        .agent_type_write
        .as_ref()
        .map(|write| write.recorded_at.clone())
        .filter(|recorded_at| !recorded_at.is_empty())
        .or_else(|| (!checkpoint.sealed_at.is_empty()).then(|| checkpoint.sealed_at.clone()))
}

fn skipped_diagnostic_writes_after(
    load: &JournalLoad,
    tab_id: &str,
    selected_seq: u64,
) -> Vec<SkippedJournalRecord> {
    load.records
        .iter()
        .filter_map(|record| {
            let JournalRecord::Transition { entry } = record else {
                return None;
            };
            if entry.key.tab_id != tab_id || entry.seq <= selected_seq {
                return None;
            }
            let LifecycleTransition::AgentTypeWrite {
                origin,
                nonce_verified,
                applied,
                ..
            } = &entry.transition
            else {
                return None;
            };
            let reason = if *origin == AgentTypeWriteOrigin::Parser {
                "parser-origin identity write is never a restore source"
            } else if !*applied {
                "refused identity write is not a restore source"
            } else if !*nonce_verified {
                "identity write was not nonce-verified"
            } else {
                return None;
            };
            Some(SkippedJournalRecord {
                seq: entry.seq,
                operation_id: entry.key.operation_id.clone(),
                record_type: "agent_type_write".into(),
                recorded_at: (!entry.recorded_at.is_empty()).then(|| entry.recorded_at.clone()),
                reason: reason.into(),
            })
        })
        .collect()
}

fn not_authorized(tab_id: &str) -> TabRepairError {
    TabRepairError::new(
        TabRepairErrorKind::NotAuthorized,
        format!(
            "repair of Tab {tab_id} requires that tab's nonce-verified agent or its task's live Plan owner"
        ),
    )
}

#[cfg(test)]
mod tests;
