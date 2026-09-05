use serde::Serialize;

use crate::agent::provider_identity::{provider_for_claimed_session, ProviderIdentityEvidence};
use crate::multiplexer::types::{Tab, TabBacking};
use crate::service::hierarchy::report_plan_ctx::tab_holds;
use crate::session::lifecycle::{
    checkpoint_history, AgentTypeWriteOrigin, JournalLoad, JournalRecord, LifecycleCheckpoint,
    LifecycleTransition, TabLifecycleJournal, TailStatus,
};

use super::provider_identity::{AgentTypeWriteOutcome, TabAgentTypeState};
use super::{AuthenticatedPlanCaller, ServiceContext};

pub(crate) const TAB_REPAIR_VERIFIED_SESSION_REQUIRED: &str =
    "tab_repair_verified_session_required";
pub(crate) const TAB_REPAIR_NOT_AUTHORIZED: &str = "tab_repair_not_authorized";
pub(crate) const TAB_REPAIR_EVIDENCE_CHANGED: &str = "tab_repair_evidence_changed";
pub(crate) const TAB_REPAIR_LIVE_PTY_REQUIRED: &str = "tab_repair_live_pty_required";
pub(crate) const TAB_REPAIR_JOURNAL_UNAVAILABLE: &str = "tab_repair_journal_unavailable";
pub(crate) const TAB_REPAIR_JOURNAL_SOURCE_MISSING: &str = "tab_repair_journal_source_missing";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabRepairErrorKind {
    NotFound,
    NotAuthorized,
    VerifiedSessionRequired,
    EvidenceChanged,
    LivePtyRequired,
    JournalUnavailable,
    JournalSourceMissing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TabRepairError {
    pub(crate) kind: TabRepairErrorKind,
    pub(crate) message: String,
}

impl TabRepairError {
    pub(crate) fn new(kind: TabRepairErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub(crate) fn code(&self) -> &'static str {
        match self.kind {
            TabRepairErrorKind::NotFound => "tab_repair_target_not_found",
            TabRepairErrorKind::NotAuthorized => TAB_REPAIR_NOT_AUTHORIZED,
            TabRepairErrorKind::VerifiedSessionRequired => TAB_REPAIR_VERIFIED_SESSION_REQUIRED,
            TabRepairErrorKind::EvidenceChanged => TAB_REPAIR_EVIDENCE_CHANGED,
            TabRepairErrorKind::LivePtyRequired => TAB_REPAIR_LIVE_PTY_REQUIRED,
            TabRepairErrorKind::JournalUnavailable => TAB_REPAIR_JOURNAL_UNAVAILABLE,
            TabRepairErrorKind::JournalSourceMissing => TAB_REPAIR_JOURNAL_SOURCE_MISSING,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AuthorizedRepairTarget {
    pub(crate) tab_id: String,
    pub(crate) identity: ProviderIdentityEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TabRepairSource {
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
pub(crate) struct JournalRepairReport {
    pub(crate) restored_from: JournalRecordReport,
    pub(crate) skipped: Vec<SkippedJournalRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct TabRepairResponse {
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
    pub(crate) fn from_identity_write(
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

fn live_task_plan(svc: &ServiceContext, tab: &Tab) -> Option<String> {
    let binding = tab.task_binding.as_ref()?;
    let plan_id = binding.plan_id.as_deref()?;
    let task_id = binding.task_id.as_deref()?;
    let plan = svc.plan_store.get(plan_id)?;
    let task = plan
        .tasks
        .iter()
        .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))?;
    tab_holds(task, &tab.id).then(|| plan_id.to_string())
}

impl ServiceContext {
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
        if !matches!(&tab.backing, TabBacking::Pty) || tab.pty_id.is_none() {
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

    pub(crate) async fn repair_tab_from_live_evidence(
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
        let (session_id, provider) = self.verified_provider_for_repair(&target.identity).await?;
        if asserted_provider.is_some_and(|asserted| asserted != provider.as_str()) {
            return Err(TabRepairError::new(
                TabRepairErrorKind::VerifiedSessionRequired,
                format!(
                    "bound session '{session_id}' verifies as provider '{provider}', not '{}'",
                    asserted_provider.unwrap_or_default()
                ),
            ));
        }
        let fresh = self.authorize_tab_repair(&target.tab_id, caller)?;
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
        self.emit_state_changed();
        Ok(response)
    }

    pub(crate) async fn repair_tab_from_journal(
        &self,
        target: AuthorizedRepairTarget,
        caller: &AuthenticatedPlanCaller,
    ) -> Result<TabRepairResponse, TabRepairError> {
        let (session_id, provider) = self.verified_provider_for_repair(&target.identity).await?;
        let inspected_tab = target.tab_id.clone();
        let inspected_session = session_id.clone();
        let inspected_provider = provider.clone();
        let plan = tokio::task::spawn_blocking(move || {
            let load = TabLifecycleJournal::shared().load();
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
        let fresh = self.authorize_tab_repair(&target.tab_id, caller)?;
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
        self.emit_state_changed();
        Ok(response)
    }

    pub(crate) async fn verified_provider_for_repair(
        &self,
        evidence: &ProviderIdentityEvidence,
    ) -> Result<(String, String), TabRepairError> {
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
mod tests {
    use super::*;
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::{AgentRole, TaskBinding};
    use crate::plan::{OrchestratorRef, Plan};
    use crate::service::plan::NewTask;
    use crate::session::lifecycle::{
        AgentTypeWriteAudit, CommittedAuthority, JournalRecord, LifecycleCheckpoint,
        LifecycleEpoch, LifecycleKey,
    };

    fn claimed_tab(provider: &str) -> Tab {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.pty_id = Some("pty-live".into());
        tab.cwd = Some("D:/repo".into());
        tab.agent_type = Some(provider.into());
        tab.agent_type_locked = true;
        tab.agent_session_id = Some("session-live".into());
        tab.session_binding_nonce = Some("nonce-live".into());
        tab.reported_session_binding_nonce = Some("nonce-live".into());
        tab.clone()
    }

    fn repair_fixture(
        provider: &str,
    ) -> (
        crate::service::test_fixture::ServiceFixture,
        String,
        AuthenticatedPlanCaller,
    ) {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.pty_id = Some("pty-live".into());
        tab.cwd = Some("D:/repo".into());
        tab.agent_type = Some(provider.into());
        tab.agent_type_locked = true;
        tab.agent_session_id = Some("session-live".into());
        tab.session_binding_nonce = Some("nonce-live".into());
        tab.reported_session_binding_nonce = Some("nonce-live".into());
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        fixture.service.agent_registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::with_session("codex", "session-live"),
        ));
        let caller = AuthenticatedPlanCaller {
            tab_id: Some(tab_id.clone()),
            session_id: Some("session-live".into()),
            binding_nonce: Some("nonce-live".into()),
        };
        (fixture, tab_id, caller)
    }

    fn checkpoint(
        seq: u64,
        operation_id: &str,
        provider: &str,
        write: Option<AgentTypeWriteAudit>,
    ) -> JournalRecord {
        JournalRecord::Checkpoint {
            checkpoint: LifecycleCheckpoint {
                key: LifecycleKey::new("tab-live", LifecycleEpoch::first(), operation_id),
                committed_through: seq,
                authority: CommittedAuthority {
                    provider: provider.into(),
                    session_id: "session-live".into(),
                    binding_nonce: format!("nonce-{seq}"),
                    role: None,
                    role_profile: None,
                    team_id: None,
                    parent_tab_id: None,
                    can_spawn: false,
                    depth: 0,
                },
                agent_type_write: write,
                prior_checkpoints: Vec::new(),
                sealed_at: format!("recorded-{seq}"),
            },
        }
    }

    #[test]
    fn own_agent_authority_requires_its_live_session_and_nonce() {
        let tab = claimed_tab("claude");
        let own = AuthenticatedPlanCaller {
            tab_id: Some(tab.id.clone()),
            session_id: tab.agent_session_id.clone(),
            binding_nonce: tab.session_binding_nonce.clone(),
        };
        let foreign = AuthenticatedPlanCaller {
            tab_id: Some("foreign".into()),
            ..own.clone()
        };
        let nonce_only = AuthenticatedPlanCaller {
            session_id: None,
            ..own.clone()
        };

        assert!(caller_matches_tab(&tab, &own));
        assert!(caller_matches_tab(&tab, &nonce_only));
        assert!(!caller_matches_tab(&tab, &foreign));
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    fn foreign_tab_without_plan_ownership_is_refused_by_constant() {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.pty_id = Some("pty-live".into());
        tab.cwd = Some("D:/repo".into());
        tab.agent_type = Some("claude".into());
        tab.agent_session_id = Some("session-live".into());
        tab.session_binding_nonce = Some("nonce-live".into());
        tab.reported_session_binding_nonce = Some("nonce-live".into());
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let foreign = AuthenticatedPlanCaller {
            tab_id: Some("foreign-tab".into()),
            session_id: Some("foreign-session".into()),
            binding_nonce: Some("foreign-nonce".into()),
        };

        let error = fixture
            .service
            .authorize_tab_repair(&tab_id, &foreign)
            .unwrap_err();

        assert_eq!(error.code(), TAB_REPAIR_NOT_AUTHORIZED);
        assert_eq!(
            fixture
                .service
                .mux
                .read()
                .get_tab(&tab_id)
                .unwrap()
                .agent_type
                .as_deref(),
            Some("claude")
        );
    }

    #[tokio::test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    async fn live_plan_owner_may_repair_its_assigned_worker() {
        let (mut mux, owner_id) = MultiplexerState::new();
        mux.set_orchestrator_role(&owner_id).unwrap();
        let workspace_id = mux.list_workspaces()[0].id.clone();
        let worker_id = mux.create_tab(&workspace_id).unwrap();
        mux.set_tab_role(&worker_id, AgentRole::Worker, Some(&owner_id))
            .unwrap();
        for (tab_id, session_id, nonce) in [
            (&owner_id, "owner-session", "owner-nonce"),
            (&worker_id, "worker-session", "worker-nonce"),
        ] {
            let tab = mux.get_tab_mut(tab_id).unwrap();
            tab.pty_id = Some(format!("pty-{tab_id}"));
            tab.cwd = Some("D:/repo".into());
            tab.agent_type = Some(if tab_id == &worker_id {
                "claude".into()
            } else {
                "codex".into()
            });
            tab.agent_type_locked = tab_id == &worker_id;
            tab.agent_session_id = Some(session_id.into());
            tab.session_binding_nonce = Some(nonce.into());
            tab.reported_session_binding_nonce = Some(nonce.into());
        }
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        fixture.service.agent_registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::with_session("codex", "worker-session"),
        ));
        let mut plan = Plan::new(
            "Repair authority".into(),
            fixture.temp.path().display().to_string(),
            OrchestratorRef::from_tab(&owner_id),
        );
        fixture.service.plan_store.save(&mut plan, 1).unwrap();
        let task_id = fixture
            .service
            .plan_store
            .add_task(
                &plan.id,
                NewTask {
                    name: "Worker repair".into(),
                    kind: "implement".into(),
                    body: "repair identity".into(),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        fixture
            .service
            .plan_store
            .assign_task(&plan.id, &task_id, &worker_id, None, 3)
            .unwrap();
        let task_binding = TaskBinding {
            plan_id: Some(plan.id),
            task_id: Some(task_id),
            spawn_handle: None,
            bound_at: 3,
        };
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&worker_id)
            .unwrap()
            .task_binding = Some(task_binding);
        let caller = AuthenticatedPlanCaller {
            tab_id: Some(owner_id),
            session_id: Some("owner-session".into()),
            binding_nonce: Some("owner-nonce".into()),
        };

        let authorized = fixture
            .service
            .authorize_tab_repair(&worker_id, &caller)
            .unwrap();
        let response = fixture
            .service
            .repair_tab_from_live_evidence(authorized, &caller, None)
            .await
            .unwrap();

        assert_eq!(response.provider.current.as_deref(), Some("codex"));
        let mux = fixture.service.mux.read();
        let worker = mux.get_tab(&worker_id).unwrap();
        assert_eq!(worker.agent_type.as_deref(), Some("codex"));
        assert_eq!(worker.agent_session_id.as_deref(), Some("worker-session"));
        assert!(!worker.agent_type_locked);
    }

    #[tokio::test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    async fn repair_from_live_evidence_corrects_locked_label_without_restart() {
        let (fixture, tab_id, caller) = repair_fixture("claude");
        let target = fixture
            .service
            .authorize_tab_repair(&tab_id, &caller)
            .unwrap();

        let response = fixture
            .service
            .repair_tab_from_live_evidence(target, &caller, None)
            .await
            .unwrap();

        assert_eq!(response.provider.status, RepairFieldStatus::Corrected);
        assert_eq!(response.parser_lock.current, Some(false));
        let mux = fixture.service.mux.read();
        let repaired = mux.get_tab(&tab_id).unwrap();
        assert_eq!(repaired.pty_id.as_deref(), Some("pty-live"));
        assert_eq!(repaired.agent_type.as_deref(), Some("codex"));
        assert_eq!(repaired.agent_session_id.as_deref(), Some("session-live"));
        assert!(!repaired.agent_type_locked);
    }

    #[tokio::test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    async fn asserted_provider_mismatch_is_refused_by_the_stable_constant() {
        let (fixture, tab_id, caller) = repair_fixture("claude");
        let target = fixture
            .service
            .authorize_tab_repair(&tab_id, &caller)
            .unwrap();

        let error = fixture
            .service
            .repair_tab_from_live_evidence(target, &caller, Some("claude"))
            .await
            .unwrap_err();

        assert_eq!(error.code(), TAB_REPAIR_VERIFIED_SESSION_REQUIRED);
        let tab = fixture.service.mux.read();
        let unchanged = tab.get_tab(&tab_id).unwrap();
        assert_eq!(unchanged.agent_type.as_deref(), Some("claude"));
        assert!(unchanged.agent_type_locked);
    }

    #[test]
    fn repair_result_reports_locked_label_correction_without_a_restart() {
        let previous = TabAgentTypeState {
            provider: Some("claude".into()),
            locked: true,
        };
        let current = TabAgentTypeState {
            provider: Some("codex".into()),
            locked: false,
        };
        let response = TabRepairResponse::from_identity_write(
            "tab-live".into(),
            TabRepairSource::LiveEvidence,
            Some("session-live".into()),
            "session-live".into(),
            "codex".into(),
            true,
            AgentTypeWriteOutcome::Applied { previous, current },
            None,
        )
        .unwrap();

        assert_eq!(response.provider.status, RepairFieldStatus::Corrected);
        assert_eq!(response.provider.current.as_deref(), Some("codex"));
        assert_eq!(response.session.status, RepairFieldStatus::Kept);
        assert_eq!(response.parser_lock.current, Some(false));
    }

    #[test]
    fn changed_evidence_is_a_typed_refusal() {
        let current = TabAgentTypeState {
            provider: Some("claude".into()),
            locked: true,
        };
        let evidence = ProviderIdentityEvidence::capture(&claimed_tab("claude"));
        let error = TabRepairResponse::from_identity_write(
            "tab-live".into(),
            TabRepairSource::LiveEvidence,
            Some("session-live".into()),
            "session-live".into(),
            "codex".into(),
            true,
            AgentTypeWriteOutcome::EvidenceChanged {
                expected: evidence.clone(),
                current_evidence: evidence,
                current,
            },
            None,
        )
        .unwrap_err();

        assert_eq!(error.code(), TAB_REPAIR_EVIDENCE_CHANGED);
    }

    #[test]
    fn unverified_session_is_refused_by_the_stable_constant() {
        let tab = claimed_tab("claude");
        let mut evidence = ProviderIdentityEvidence::capture(&tab);
        evidence.reported_nonce = Some("foreign-nonce".into());

        let error = verified_session_query(&evidence).unwrap_err();

        assert_eq!(error.code(), TAB_REPAIR_VERIFIED_SESSION_REQUIRED);
    }

    #[test]
    fn journal_restore_skips_newer_wrong_provider_and_names_the_source() {
        let load = JournalLoad {
            records: vec![
                checkpoint(10, "good-operation", "codex", None),
                checkpoint(20, "corrupt-operation", "claude", None),
            ],
            tail: TailStatus::Intact,
        };

        let plan = select_journal_restore(&load, "tab-live", "session-live", "codex").unwrap();

        assert_eq!(plan.provider, "codex");
        assert_eq!(plan.report.restored_from.seq, 10);
        assert_eq!(plan.report.restored_from.operation_id, "good-operation");
        assert_eq!(plan.report.skipped.len(), 1);
        assert_eq!(plan.report.skipped[0].seq, 20);
        assert!(plan.report.skipped[0].reason.contains("does not own"));
    }

    #[test]
    fn parser_origin_checkpoint_is_never_a_restore_source() {
        let parser_write = AgentTypeWriteAudit {
            origin: AgentTypeWriteOrigin::Parser,
            previous_provider: Some("codex".into()),
            new_provider: Some("codex".into()),
            attempted_provider: Some("claude".into()),
            caller_tab_id: None,
            session_id: Some("session-live".into()),
            nonce_verified: true,
            applied: false,
            recorded_at: "parser-write".into(),
        };
        let load = JournalLoad {
            records: vec![
                checkpoint(10, "good-operation", "codex", None),
                checkpoint(20, "parser-operation", "codex", Some(parser_write)),
            ],
            tail: TailStatus::Intact,
        };

        let plan = select_journal_restore(&load, "tab-live", "session-live", "codex").unwrap();

        assert_eq!(plan.report.restored_from.seq, 10);
        assert_eq!(plan.report.skipped[0].seq, 20);
        assert!(plan.report.skipped[0].reason.contains("parser-origin"));
    }

    #[test]
    fn nonce_verified_repair_checkpoint_is_a_restore_source() {
        let repair_write = AgentTypeWriteAudit {
            origin: AgentTypeWriteOrigin::Repair,
            previous_provider: Some("claude".into()),
            new_provider: Some("codex".into()),
            attempted_provider: Some("codex".into()),
            caller_tab_id: Some("tab-live".into()),
            session_id: Some("session-live".into()),
            nonce_verified: true,
            applied: true,
            recorded_at: "repair-write".into(),
        };
        let load = JournalLoad {
            records: vec![checkpoint(
                20,
                "repair-operation",
                "codex",
                Some(repair_write),
            )],
            tail: TailStatus::Intact,
        };

        let plan = select_journal_restore(&load, "tab-live", "session-live", "codex").unwrap();

        assert_eq!(plan.report.restored_from.seq, 20);
        assert_eq!(
            plan.report.restored_from.recorded_at.as_deref(),
            Some("repair-write")
        );
        assert!(plan.report.skipped.is_empty());
    }
}
