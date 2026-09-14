use std::collections::HashMap;

use super::record::{
    AgentTypeWriteAudit, AgentTypeWriteOrigin, CommittedAuthority, JournalRecord, JournalSeq,
    LifecycleCheckpoint, LifecycleCheckpointHistoryEntry, LifecycleKey, LifecycleTransition,
};

pub const PRIOR_CHECKPOINT_LIMIT: usize = 16;

/// Authority accumulated by one in-flight operation. It becomes a checkpoint
/// only when the operation seals with every required field committed.
#[derive(Debug, Clone)]
struct OperationBuilder {
    key: LifecycleKey,
    provider: Option<String>,
    session: Option<(String, String)>,
    role: Option<(Option<String>, Option<String>, bool, u8)>,
    team: Option<(Option<String>, Option<String>)>,
    agent_type_write: Option<AgentTypeWriteAudit>,
    prior_checkpoints: Vec<LifecycleCheckpointHistoryEntry>,
    sealed_through: Option<JournalSeq>,
    sealed_at: String,
    abandoned: bool,
}

impl OperationBuilder {
    fn new(key: LifecycleKey) -> Self {
        Self {
            key,
            provider: None,
            session: None,
            role: None,
            team: None,
            agent_type_write: None,
            prior_checkpoints: Vec::new(),
            sealed_through: None,
            sealed_at: String::new(),
            abandoned: false,
        }
    }

    fn from_checkpoint(checkpoint: &LifecycleCheckpoint) -> Self {
        Self {
            key: checkpoint.key.clone(),
            provider: Some(checkpoint.authority.provider.clone()),
            session: Some((
                checkpoint.authority.session_id.clone(),
                checkpoint.authority.binding_nonce.clone(),
            )),
            role: Some((
                checkpoint.authority.role.clone(),
                checkpoint.authority.role_profile.clone(),
                checkpoint.authority.can_spawn,
                checkpoint.authority.depth,
            )),
            team: Some((
                checkpoint.authority.team_id.clone(),
                checkpoint.authority.parent_tab_id.clone(),
            )),
            agent_type_write: checkpoint.agent_type_write.clone(),
            prior_checkpoints: checkpoint.prior_checkpoints.clone(),
            sealed_through: Some(checkpoint.committed_through),
            sealed_at: checkpoint.sealed_at.clone(),
            abandoned: false,
        }
    }

    /// Authority changed inside an already-sealed operation still commits; the
    /// checkpoint advances to the sequence that proved it.
    fn reseal_at(&mut self, seq: JournalSeq) {
        if self.sealed_through.is_some() {
            self.sealed_through = Some(seq);
        }
    }

    /// Provider identity plus a nonce-proven session are the floor. A role or
    /// Team binding without them is residue, never authority.
    fn checkpoint(&self) -> Option<LifecycleCheckpoint> {
        if self.abandoned {
            return None;
        }
        let committed_through = self.sealed_through?;
        let provider = self.provider.clone()?;
        let (session_id, binding_nonce) = self.session.clone()?;
        let (role, role_profile, can_spawn, depth) =
            self.role.clone().unwrap_or((None, None, false, 0));
        let (team_id, parent_tab_id) = self.team.clone().unwrap_or((None, None));
        Some(LifecycleCheckpoint {
            key: self.key.clone(),
            committed_through,
            authority: CommittedAuthority {
                provider,
                session_id,
                binding_nonce,
                role,
                role_profile,
                team_id,
                parent_tab_id,
                can_spawn,
                depth,
            },
            agent_type_write: self.agent_type_write.clone(),
            prior_checkpoints: self.prior_checkpoints.clone(),
            sealed_at: self.sealed_at.clone(),
        })
    }
}

#[derive(Default)]
struct CheckpointReducer {
    sealed: HashMap<String, LifecycleCheckpoint>,
    open: HashMap<String, OperationBuilder>,
}

impl CheckpointReducer {
    fn insert_checkpoint(&mut self, mut checkpoint: LifecycleCheckpoint) {
        if let Some(previous) = self.sealed.get(&checkpoint.key.tab_id) {
            checkpoint = carry_checkpoint_history(checkpoint, previous);
        }
        self.sealed
            .insert(checkpoint.key.tab_id.clone(), checkpoint);
    }

    fn ingest(&mut self, record: &JournalRecord) {
        match record {
            JournalRecord::Checkpoint { checkpoint } => {
                self.insert_checkpoint(checkpoint.clone());
                self.open.remove(&checkpoint.key.tab_id);
            }
            JournalRecord::Transition { entry } => {
                let tab_id = entry.key.tab_id.clone();
                let builder = match self.open.get_mut(&tab_id) {
                    Some(builder) if builder.key == entry.key => builder,
                    _ => {
                        let builder = self
                            .sealed
                            .get(&tab_id)
                            .filter(|checkpoint| checkpoint.key == entry.key)
                            .map(OperationBuilder::from_checkpoint)
                            .unwrap_or_else(|| OperationBuilder::new(entry.key.clone()));
                        self.open.insert(tab_id.clone(), builder);
                        self.open.get_mut(&tab_id).expect("builder just inserted")
                    }
                };
                apply(builder, entry.seq, &entry.transition, &entry.recorded_at);
                match &entry.transition {
                    LifecycleTransition::Closed { .. } => {
                        self.sealed.remove(&tab_id);
                        self.open.remove(&tab_id);
                    }
                    LifecycleTransition::RolledBack { .. } => {
                        self.sealed.remove(&tab_id);
                    }
                    LifecycleTransition::AgentTypeWrite { .. }
                    | LifecycleTransition::PermissionAutoConfirm { .. }
                    | LifecycleTransition::RestoreAwaitingAgentRestart { .. } => {}
                    _ => {
                        if let Some(checkpoint) = self
                            .open
                            .get(&tab_id)
                            .and_then(OperationBuilder::checkpoint)
                        {
                            self.insert_checkpoint(checkpoint);
                        }
                    }
                }
            }
        }
    }
}

fn carry_checkpoint_history(
    mut checkpoint: LifecycleCheckpoint,
    previous: &LifecycleCheckpoint,
) -> LifecycleCheckpoint {
    if checkpoint == *previous {
        return checkpoint;
    }
    let inherited = std::mem::take(&mut checkpoint.prior_checkpoints);
    let mut history = Vec::with_capacity(PRIOR_CHECKPOINT_LIMIT);
    let previous_identity = (!same_identity_authority(&checkpoint, previous))
        .then(|| previous.history_entry())
        .into_iter();
    for entry in previous_identity
        .chain(previous.prior_checkpoints.iter().cloned())
        .chain(inherited)
    {
        if !history.contains(&entry) {
            history.push(entry);
        }
        if history.len() == PRIOR_CHECKPOINT_LIMIT {
            break;
        }
    }
    checkpoint.prior_checkpoints = history;
    checkpoint
}

fn same_identity_authority(left: &LifecycleCheckpoint, right: &LifecycleCheckpoint) -> bool {
    left.key == right.key
        && left.authority.provider == right.authority.provider
        && left.authority.session_id == right.authority.session_id
        && left.authority.binding_nonce == right.authority.binding_nonce
        && same_identity_write_source(
            left.agent_type_write.as_ref(),
            right.agent_type_write.as_ref(),
        )
}

fn same_identity_write_source(
    left: Option<&AgentTypeWriteAudit>,
    right: Option<&AgentTypeWriteAudit>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            left.origin == right.origin
                && left.new_provider == right.new_provider
                && left.session_id == right.session_id
                && left.nonce_verified == right.nonce_verified
                && left.applied == right.applied
        }
        (None, None) => true,
        _ => false,
    }
}

/// Newest sealed checkpoint per tab, replayed in journal order. An unsealed
/// newer operation leaves the previous checkpoint standing under its own epoch.
pub fn sealed_checkpoints(records: &[JournalRecord]) -> Vec<LifecycleCheckpoint> {
    let mut reducer = CheckpointReducer::default();
    for record in records {
        reducer.ingest(record);
    }

    let mut checkpoints: Vec<_> = reducer.sealed.into_values().collect();
    checkpoints.sort_by(|left, right| left.key.tab_id.cmp(&right.key.tab_id));
    checkpoints
}

pub fn checkpoint_history(records: &[JournalRecord], tab_id: &str) -> Vec<LifecycleCheckpoint> {
    let Some(checkpoint) = sealed_checkpoints(records)
        .into_iter()
        .find(|checkpoint| checkpoint.key.tab_id == tab_id)
    else {
        return Vec::new();
    };
    let mut history = Vec::with_capacity(checkpoint.prior_checkpoints.len() + 1);
    history.push(checkpoint.clone());
    history.extend(
        checkpoint
            .prior_checkpoints
            .iter()
            .map(LifecycleCheckpointHistoryEntry::checkpoint),
    );
    history
}

fn apply(
    builder: &mut OperationBuilder,
    seq: JournalSeq,
    transition: &LifecycleTransition,
    recorded_at: &str,
) {
    match transition {
        LifecycleTransition::ProviderIdentityCommitted { provider } => {
            builder.provider = Some(provider.clone());
        }
        LifecycleTransition::AgentTypeWrite {
            origin,
            previous_provider,
            new_provider,
            attempted_provider,
            caller_tab_id,
            session_id,
            nonce_verified,
            applied,
        } => {
            if *applied && *nonce_verified && *origin != AgentTypeWriteOrigin::Parser {
                builder.agent_type_write = Some(AgentTypeWriteAudit {
                    origin: *origin,
                    previous_provider: previous_provider.clone(),
                    new_provider: new_provider.clone(),
                    attempted_provider: attempted_provider.clone(),
                    caller_tab_id: caller_tab_id.clone(),
                    session_id: session_id.clone(),
                    nonce_verified: *nonce_verified,
                    applied: *applied,
                    recorded_at: recorded_at.to_string(),
                });
            }
        }
        LifecycleTransition::SessionBound {
            session_id,
            binding_nonce,
            ..
        } => {
            builder.session = Some((session_id.clone(), binding_nonce.clone()));
            builder.reseal_at(seq);
        }
        LifecycleTransition::RoleBinding {
            role,
            role_profile,
            can_spawn,
            depth,
        } => {
            builder.role = Some((role.clone(), role_profile.clone(), *can_spawn, *depth));
            builder.reseal_at(seq);
        }
        LifecycleTransition::TeamBinding {
            team_id,
            parent_tab_id,
        } => {
            builder.team = Some((team_id.clone(), parent_tab_id.clone()));
            builder.reseal_at(seq);
        }
        LifecycleTransition::LaunchReady => {
            builder.sealed_through = Some(seq);
            builder.sealed_at = recorded_at.to_string();
        }
        LifecycleTransition::RolledBack { .. } | LifecycleTransition::Closed { .. } => {
            builder.abandoned = true;
            builder.sealed_through = None;
        }
        LifecycleTransition::EpochOpened
        | LifecycleTransition::RestartRequested
        | LifecycleTransition::RestoreAwaitingAgentRestart { .. }
        | LifecycleTransition::PermissionAutoConfirm { .. } => {}
    }
}

/// Surviving checkpoints plus the transitions no checkpoint covers. Replaying
/// this yields exactly the checkpoints the full history yields.
pub fn compact_records(records: &[JournalRecord]) -> Vec<JournalRecord> {
    let checkpoints = sealed_checkpoints(records);
    let covered: HashMap<&str, JournalSeq> = checkpoints
        .iter()
        .map(|checkpoint| (checkpoint.key.tab_id.as_str(), checkpoint.committed_through))
        .collect();

    let closed = closed_tabs(records);
    let retained_agent_type_writes = retained_agent_type_write_sequences(records);
    let retained_permission_audits = retained_permission_auto_confirm_sequences(records);
    let mut compacted: Vec<JournalRecord> = checkpoints
        .iter()
        .cloned()
        .map(|checkpoint| JournalRecord::Checkpoint { checkpoint })
        .collect();
    for record in records {
        let JournalRecord::Transition { entry } = record else {
            continue;
        };
        if closed.contains(entry.key.tab_id.as_str()) {
            continue;
        }
        let is_covered = covered
            .get(entry.key.tab_id.as_str())
            .is_some_and(|sealed_through| entry.seq <= *sealed_through);
        let is_agent_type_write = matches!(
            &entry.transition,
            LifecycleTransition::AgentTypeWrite { .. }
        );
        let is_permission_audit = matches!(
            &entry.transition,
            LifecycleTransition::PermissionAutoConfirm { .. }
        );
        if (is_agent_type_write && retained_agent_type_writes.contains(&entry.seq))
            || (is_permission_audit && retained_permission_audits.contains(&entry.seq))
            || (!is_agent_type_write && !is_permission_audit && !is_covered)
        {
            compacted.push(record.clone());
        }
    }
    compacted.sort_by_key(JournalRecord::seq);
    compacted
}

/// Per-tab audit rows kept through compaction; older ones are telemetry only.
pub const PERMISSION_AUTO_CONFIRM_AUDIT_LIMIT: usize = 20;

fn retained_permission_auto_confirm_sequences(
    records: &[JournalRecord],
) -> std::collections::HashSet<JournalSeq> {
    let mut per_tab = HashMap::<&str, Vec<JournalSeq>>::new();
    for record in records {
        let JournalRecord::Transition { entry } = record else {
            continue;
        };
        if matches!(
            &entry.transition,
            LifecycleTransition::PermissionAutoConfirm { .. }
        ) {
            per_tab
                .entry(entry.key.tab_id.as_str())
                .or_default()
                .push(entry.seq);
        }
    }
    per_tab
        .into_values()
        .flat_map(|mut seqs| {
            seqs.sort_unstable();
            let skip = seqs
                .len()
                .saturating_sub(PERMISSION_AUTO_CONFIRM_AUDIT_LIMIT);
            seqs.into_iter().skip(skip)
        })
        .collect()
}

fn retained_agent_type_write_sequences(
    records: &[JournalRecord],
) -> std::collections::HashSet<JournalSeq> {
    let mut latest_attempt = HashMap::<&str, JournalSeq>::new();
    for record in records {
        let JournalRecord::Transition { entry } = record else {
            continue;
        };
        if matches!(
            &entry.transition,
            LifecycleTransition::AgentTypeWrite { .. }
        ) {
            latest_attempt.insert(entry.key.tab_id.as_str(), entry.seq);
        }
    }
    latest_attempt.into_values().collect()
}

/// Tabs whose last recorded transition is `Closed`. They can never produce a
/// checkpoint again, so compaction drops their history instead of growing it.
fn closed_tabs(records: &[JournalRecord]) -> std::collections::HashSet<&str> {
    let mut closed = std::collections::HashSet::new();
    for record in records {
        match record {
            JournalRecord::Transition { entry } => match &entry.transition {
                LifecycleTransition::Closed { .. } => {
                    closed.insert(entry.key.tab_id.as_str());
                }
                _ => {
                    closed.remove(entry.key.tab_id.as_str());
                }
            },
            JournalRecord::Checkpoint { checkpoint } => {
                closed.remove(checkpoint.key.tab_id.as_str());
            }
        }
    }
    closed
}
