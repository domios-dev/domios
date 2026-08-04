use std::collections::HashMap;

use super::record::{
    CommittedAuthority, JournalRecord, JournalSeq, LifecycleCheckpoint, LifecycleKey,
    LifecycleTransition,
};

/// Authority accumulated by one in-flight operation. It becomes a checkpoint
/// only when the operation seals with every required field committed.
#[derive(Debug, Clone)]
struct OperationBuilder {
    key: LifecycleKey,
    provider: Option<String>,
    session: Option<(String, String)>,
    role: Option<(Option<String>, Option<String>, bool, u8)>,
    team: Option<(Option<String>, Option<String>)>,
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
            sealed_through: None,
            sealed_at: String::new(),
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
            sealed_at: self.sealed_at.clone(),
        })
    }
}

/// Replay records in journal order and return the newest sealed checkpoint per
/// tab. A tab whose newest operation never sealed has no checkpoint — the
/// previous operation's checkpoint is kept on record, but a snapshot from a
/// different operation can never match it.
pub fn sealed_checkpoints(records: &[JournalRecord]) -> Vec<LifecycleCheckpoint> {
    let mut sealed: HashMap<String, LifecycleCheckpoint> = HashMap::new();
    let mut open: HashMap<String, OperationBuilder> = HashMap::new();

    for record in records {
        match record {
            JournalRecord::Checkpoint { checkpoint } => {
                sealed.insert(checkpoint.key.tab_id.clone(), checkpoint.clone());
                open.remove(&checkpoint.key.tab_id);
            }
            JournalRecord::Transition { entry } => {
                let tab_id = entry.key.tab_id.clone();
                let builder = match open.get_mut(&tab_id) {
                    Some(builder) if builder.key == entry.key => builder,
                    _ => {
                        open.insert(tab_id.clone(), OperationBuilder::new(entry.key.clone()));
                        open.get_mut(&tab_id).expect("builder just inserted")
                    }
                };
                apply(builder, entry.seq, &entry.transition, &entry.recorded_at);
                match &entry.transition {
                    LifecycleTransition::Closed => {
                        sealed.remove(&tab_id);
                        open.remove(&tab_id);
                    }
                    LifecycleTransition::RolledBack { .. } => {
                        sealed.remove(&tab_id);
                    }
                    _ => {
                        if let Some(checkpoint) =
                            open.get(&tab_id).and_then(OperationBuilder::checkpoint)
                        {
                            sealed.insert(tab_id, checkpoint);
                        }
                    }
                }
            }
        }
    }

    let mut checkpoints: Vec<_> = sealed.into_values().collect();
    checkpoints.sort_by(|left, right| left.key.tab_id.cmp(&right.key.tab_id));
    checkpoints
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
        LifecycleTransition::SessionBound {
            session_id,
            binding_nonce,
        } => {
            builder.session = Some((session_id.clone(), binding_nonce.clone()));
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
        LifecycleTransition::RolledBack { .. } | LifecycleTransition::Closed => {
            builder.abandoned = true;
            builder.sealed_through = None;
        }
        LifecycleTransition::EpochOpened | LifecycleTransition::RestartRequested => {}
    }
}

/// Compaction output: every surviving checkpoint, then the transitions that
/// belong to operations no checkpoint covers. Replaying it yields exactly the
/// checkpoints the full history yields.
pub fn compact_records(records: &[JournalRecord]) -> Vec<JournalRecord> {
    let checkpoints = sealed_checkpoints(records);
    let covered: HashMap<&str, JournalSeq> = checkpoints
        .iter()
        .map(|checkpoint| {
            (
                checkpoint.key.tab_id.as_str(),
                checkpoint.committed_through,
            )
        })
        .collect();

    let closed = closed_tabs(records);
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
        if !is_covered {
            compacted.push(record.clone());
        }
    }
    compacted.sort_by_key(JournalRecord::seq);
    compacted
}

/// Tabs whose last recorded transition is `Closed`. They can never produce a
/// checkpoint again, so compaction drops their history instead of growing it.
fn closed_tabs(records: &[JournalRecord]) -> std::collections::HashSet<&str> {
    let mut closed = std::collections::HashSet::new();
    for record in records {
        match record {
            JournalRecord::Transition { entry } => match &entry.transition {
                LifecycleTransition::Closed => {
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
