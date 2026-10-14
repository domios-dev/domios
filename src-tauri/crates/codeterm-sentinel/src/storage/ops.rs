//! Typed Storage submissions for each staged Sentinel mutation. Cache publication happens in
//! `Store::conclude` after COMMIT; each settlement maps the committed outcome to the domain.

use super::{codec, finishing, Failure, Staged};
use crate::handlers::{ResultRecord, Transition};
use crate::source::RunBindOutcome;
use crate::source::RunBinding;
use crate::store::{Lease, ScheduleChange, Sentinel, StopNotice, Store, MAX_ACTIVE};
use codeterm_storage::{Fence, MutationOutcome, ScheduleUpdate};

fn refused<T: Send + 'static>(error: String) -> Staged<T> {
    Staged::ready(Err(error))
}

fn applied(outcome: MutationOutcome) -> bool {
    outcome == MutationOutcome::Applied
}

impl Store {
    pub fn sqlite_set_run_binding(
        &mut self,
        id: &str,
        expected_generation: u64,
        cron_id: &str,
        expected_run: Option<RunBinding>,
        run: Option<RunBinding>,
        next_run: u64,
        last_reason: Option<String>,
    ) -> Staged<RunBindOutcome> {
        let stored = match codec::id(id) {
            Ok(stored) => stored,
            Err(error) => return refused(error),
        };
        let to_record = |run: RunBinding| codeterm_storage::RunRecord {
            run_id: run.run_id,
            tab_id: run.tab_id,
            bound_at: run.bound_at,
        };
        let replaced = run.as_ref().and(expected_run.clone());
        let expected_run = expected_run.map(to_record);
        let run = run.map(to_record);
        let binding = run.is_some();
        let (cron_id, sentinel_id) = (cron_id.to_string(), id.to_string());
        let committed_id = sentinel_id.clone();
        self.submit(
            move |repo| {
                repo.set_run_binding(
                    stored,
                    expected_generation,
                    cron_id,
                    expected_run,
                    run,
                    next_run,
                    last_reason,
                )
            },
            finishing(move |store, outcome| {
                let commit = outcome.map_err(Failure::message)?;
                let applied = commit.outcome == codeterm_storage::MutationOutcome::Applied;
                if applied {
                    if let Some(cancelled) = store.active.get(&committed_id) {
                        cancelled.cancel();
                    }
                    if binding {
                        store.wake_pending.insert(committed_id.clone());
                    } else {
                        store.wake_pending.remove(&committed_id);
                    }
                }
                Ok(RunBindOutcome {
                    changed: applied,
                    replaced: applied.then_some(replaced).flatten(),
                })
            }),
        )
    }

    pub fn sqlite_insert(
        &mut self,
        job: Sentinel,
        associate: &[String],
    ) -> Staged<Sentinel> {
        let create = match codec::create(&job, associate) {
            Ok(create) => create,
            Err(error) => return refused(error),
        };
        self.submit(
            move |repo| repo.create(create),
            finishing(move |_, outcome| {
                let commit = outcome.map_err(Failure::message)?;
                match commit.outcome {
                    MutationOutcome::Applied => Ok(job),
                    MutationOutcome::Fenced(Fence::Duplicate) => {
                        Err("duplicate blueprint ID".into())
                    }
                    MutationOutcome::Fenced(Fence::Capacity) => {
                        Err("blueprint capacity reached".into())
                    }
                    other => Err(format!("blueprint creation refused: {other:?}")),
                }
            }),
        )
    }

    pub fn sqlite_update_definition(
        &mut self,
        expect_version: u64,
        author_tab_id: &str,
        authored_at: u64,
        cause_transition_id: Option<String>,
        job: Sentinel,
    ) -> Staged<Sentinel> {
        let id = match codec::id(&job.id) {
            Ok(id) => id,
            Err(error) => return refused(error),
        };
        let update = match codec::update_definition(
            &job,
            author_tab_id.into(),
            cause_transition_id,
            authored_at,
        ) {
            Ok(update) => update,
            Err(error) => return refused(error),
        };
        let public_id = job.id.clone();
        self.submit(
            move |repo| repo.update_definition(id, expect_version, update),
            finishing(move |_, outcome| {
                let commit = outcome.map_err(Failure::message)?;
                match commit.outcome {
                    MutationOutcome::Applied => {
                        commit.sentinels.first().map(codec::decode).ok_or_else(|| {
                            "blueprint update returned no committed state".to_string()
                        })
                    }
                    MutationOutcome::Fenced(Fence::Version) => {
                        let current_version = commit
                            .sentinels
                            .first()
                            .map(|sentinel| sentinel.record.definition.version)
                            .unwrap_or(expect_version);
                        Err(crate::amend::version_conflict(current_version))
                    }
                    MutationOutcome::Fenced(Fence::Missing) => {
                        Err(format!("blueprint not found: {public_id}"))
                    }
                    other => Err(format!("blueprint update refused: {other:?}")),
                }
            }),
        )
    }

    /// The lease is already cancelled; a failed commit keeps the stop intent in memory.
    pub fn sqlite_stop(
        &mut self,
        id: &str,
        reason: &str,
        notice: Option<StopNotice>,
    ) -> Staged<Sentinel> {
        let stored = match codec::id(id) {
            Ok(stored) => stored,
            Err(error) => return refused(error),
        };
        let (id, reason) = (id.to_string(), reason.to_string());
        let stop_reason = reason.clone();
        self.submit(
            move |repo| repo.stop(stored, stop_reason),
            finishing(move |store, outcome| match outcome {
                Ok(commit) => {
                    match commit.outcome {
                        MutationOutcome::Applied => {
                            if let Some(notice) = notice {
                                store.pending_stop_notices.push(notice);
                            }
                            commit.sentinels.first().map(codec::decode).ok_or_else(|| {
                                "blueprint stop returned no committed state".to_string()
                            })
                        }
                        MutationOutcome::Fenced(Fence::Missing) => {
                            Err(format!("blueprint not found: {id}"))
                        }
                        other => Err(format!("blueprint stop refused: {other:?}")),
                    }
                }
                Err(Failure::Failed(message)) => {
                    store.apply_stop(&id, &reason);
                    Err(message)
                }
                Err(Failure::Refused(message)) => Err(message),
            }),
        )
    }

    pub fn sqlite_remove_team(&mut self, team: &str) -> Staged<()> {
        let team = team.to_string();
        let removed = team.clone();
        self.submit(
            move |repo| repo.remove_team(removed),
            finishing(move |store, outcome| match outcome {
                Ok(_) => Ok(()),
                Err(Failure::Failed(message)) => {
                    let removed: std::collections::BTreeSet<_> = store
                        .jobs
                        .iter()
                        .filter(|job| job.team_id.as_deref() == Some(team.as_str()))
                        .map(|job| job.id.clone())
                        .collect();
                    store
                        .jobs
                        .retain(|job| job.team_id.as_deref() != Some(team.as_str()));
                    store.wake_pending.retain(|id| !removed.contains(id));
                    Err(message)
                }
                Err(Failure::Refused(message)) => Err(message),
            }),
        )
    }

    pub fn sqlite_delete_scenario(
        &mut self,
        id: &str,
        workspace: &str,
        team: Option<&str>,
    ) -> Staged<()> {
        let (stored, id, workspace, team) = match codec::id(id) {
            Ok(stored) => (
                stored,
                id.to_string(),
                workspace.to_string(),
                team.map(str::to_string),
            ),
            Err(error) => return refused(error),
        };
        self.submit(
            move |repo| repo.delete_scenario(stored, workspace, team),
            finishing(move |_store, outcome| match outcome {
                Ok(commit) if commit.outcome == MutationOutcome::Applied => Ok(()),
                Ok(commit) if commit.outcome == MutationOutcome::Fenced(Fence::Missing) => {
                    Err(format!("Sentinel scenario not found: {id}"))
                }
                Ok(commit) => Err(format!(
                    "Sentinel scenario delete refused: {:?}",
                    commit.outcome
                )),
                Err(Failure::Failed(message) | Failure::Refused(message)) => Err(message),
            }),
        )
    }

    /// Leases are issued only after their scheduling commit; a failure revokes them all.
    pub fn sqlite_due(
        &mut self,
        leases: Vec<Lease>,
        changes: Vec<ScheduleChange>,
        consumed_wakes: Vec<String>,
        stop_notices: Vec<StopNotice>,
    ) -> Staged<Vec<Lease>> {
        if changes.is_empty() {
            return Staged::ready(Ok(leases));
        }
        let mut updates = Vec::with_capacity(changes.len());
        for change in changes {
            let update = match change {
                ScheduleChange::Advance { id, next_run } => {
                    codec::id(&id).map(|id| ScheduleUpdate::Advance { id, next_run })
                }
                ScheduleChange::Stop { id, reason } => {
                    codec::id(&id).map(|id| ScheduleUpdate::Stop {
                        id,
                        reason: reason.to_string(),
                    })
                }
            };
            match update {
                Ok(update) => updates.push(update),
                Err(error) => {
                    self.wake_pending.extend(consumed_wakes);
                    return revoke(self, leases, error);
                }
            }
        }
        self.submit(
            move |repo| repo.schedule(updates),
            finishing(move |store, outcome| match outcome {
                Ok(commit) => {
                    if commit.outcome == MutationOutcome::Applied {
                        store.pending_stop_notices.extend(stop_notices);
                    }
                    for id in consumed_wakes {
                        store.wake_pending.remove(&id);
                    }
                    Ok(leases)
                }
                Err(failure) => {
                    store.wake_pending.extend(consumed_wakes);
                    for lease in &leases {
                        lease.cancelled.cancel();
                        store.active.remove(&lease.job.id);
                    }
                    Err(failure.message())
                }
            }),
        )
    }

    /// `transition` turns a lease outcome into an accepted transition in the same commit.
    pub fn sqlite_finish(
        &mut self,
        job: &Sentinel,
        transition: Option<Transition>,
    ) -> Staged<bool> {
        let stored = match codec::id(&job.id) {
            Ok(stored) => stored,
            Err(error) => return refused(error),
        };
        let outcome = codec::outcome(job);
        let generation = job.revision;
        let accepted = transition.as_ref().map(codec::new_transition);
        let intent = job.clone();
        self.submit(
            move |repo| match accepted {
                Some(accepted) => repo.accept_transition(stored, generation, outcome, accepted),
                None => repo.finish(stored, generation, outcome),
            },
            finishing(move |store, result| match result {
                Ok(commit) => Ok(applied(commit.outcome)),
                Err(Failure::Failed(message)) => {
                    store.keep_finish_intent(&intent, transition);
                    Err(message)
                }
                Err(Failure::Refused(message)) => Err(message),
            }),
        )
    }

    /// Durable claim-before-action: the claim exists only once COMMIT returned.
    pub fn sqlite_claim(
        &mut self,
        job: &Sentinel,
        transition: &str,
        claim: ResultRecord,
    ) -> Staged<Option<ResultRecord>> {
        let stored = match codec::id(&job.id) {
            Ok(stored) => stored,
            Err(error) => return refused(error),
        };
        let fence = codec::fence(job, transition);
        let record = codec::result(&claim);
        let source = job.id.clone();
        self.submit(
            move |repo| repo.claim_handler(stored, fence, record),
            finishing(move |store, outcome| match outcome {
                Ok(commit) => Ok(applied(commit.outcome).then_some(claim)),
                Err(failure) => {
                    if let Some(cancelled) = store.active.get(&source) {
                        cancelled.cancel();
                    }
                    Err(failure.message())
                }
            }),
        )
    }

    pub fn sqlite_handler_result(
        &mut self,
        source: &str,
        transition: &str,
        updated: &ResultRecord,
    ) -> Staged<()> {
        let stored = match codec::id(source) {
            Ok(stored) => stored,
            Err(error) => return refused(error),
        };
        let (transition, record) = (transition.to_string(), codec::result(updated));
        self.submit(
            move |repo| repo.finish_handler_result(stored, transition, record),
            finishing(|_, outcome| outcome.map(|_| ()).map_err(Failure::message)),
        )
    }

    pub fn sqlite_skip(
        &mut self,
        source: &str,
        transition: &str,
        skipped: &[ResultRecord],
    ) -> Staged<()> {
        if skipped.is_empty() {
            return Staged::ready(Ok(()));
        }
        let stored = match codec::id(source) {
            Ok(stored) => stored,
            Err(error) => return refused(error),
        };
        let transition = transition.to_string();
        let records = skipped.iter().map(codec::result).collect();
        self.submit(
            move |repo| repo.skip_handlers(stored, transition, records),
            finishing(|_, outcome| outcome.map(|_| ()).map_err(Failure::message)),
        )
    }

    pub fn sqlite_activate(
        &mut self,
        source: &Sentinel,
        transition: &str,
        target: &str,
        input_due_at: Option<u64>,
    ) -> Staged<bool> {
        let (stored, activated) = match (codec::id(&source.id), codec::id(target)) {
            (Ok(stored), Ok(activated)) => (stored, activated),
            (Err(error), _) | (_, Err(error)) => return refused(error),
        };
        let fence = codec::fence(source, transition);
        self.submit(
            move |repo| repo.activate_target(stored, fence, activated, MAX_ACTIVE, input_due_at),
            finishing(|_, outcome| {
                let commit = outcome.map_err(Failure::message)?;
                match commit.outcome {
                    MutationOutcome::Applied => Ok(true),
                    MutationOutcome::Unchanged => Ok(false),
                    MutationOutcome::Fenced(Fence::Capacity) => {
                        Err("blueprint active capacity reached".into())
                    }
                    MutationOutcome::Fenced(Fence::NotClaimed) => Err(
                        "blueprint activation requires its declared durable handler claim".into(),
                    ),
                    MutationOutcome::Fenced(Fence::Missing) => {
                        Err("blueprint target not found".into())
                    }
                    MutationOutcome::Fenced(Fence::Scope) => {
                        Err("blueprint target requires the same creator, Team and workspace".into())
                    }
                    MutationOutcome::Fenced(_) => Err("blueprint activation source fenced".into()),
                }
            }),
        )
    }
}

fn revoke(store: &mut Store, leases: Vec<Lease>, error: String) -> Staged<Vec<Lease>> {
    for lease in &leases {
        lease.cancelled.cancel();
        store.active.remove(&lease.job.id);
    }
    Staged::ready(Err(error))
}
