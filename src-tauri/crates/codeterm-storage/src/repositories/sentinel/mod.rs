//! Typed Sentinel repository: scenario identity, immutable definition versions, stage runtime
//! and handler claims/results. Every mutation is bounded and re-asserts its fences atomically.

mod definitions;
mod pipeline;
mod queries;
mod records;
/// `runtime::{import, remove_team_rows}` are transaction joins for integration owners.
pub(crate) mod runtime;

#[cfg(test)]
mod tests;

use crate::error::StorageError;
use crate::owner::Storage;
use crate::repositories::{to_sql_u64, Committed, Completion, Read};
use records::{identifier, overhead, text};
use std::collections::{BTreeMap, HashSet};

pub use records::{
    AmendmentRecord, BatchFence, BlueprintJournalEntry, BlueprintJournalWrite, BucketDefinition,
    CaptureRecord, CommandParameters, CommandResultRecord, CommandStatus, DefinitionProvenance,
    DefinitionUpdate, EscalateTarget, Fence, HandlerAction, HandlerDefinition, HandlerResultRecord,
    HandlerStatus, MutationOutcome, NewTransition, ObserveKind, ObserveSource, RunRecord,
    RuntimeOutcome, SafeKind, ScheduleUpdate, SentinelCommit, SentinelCreate, SentinelId,
    SentinelIdentity, SentinelImport, SentinelRecord, SentinelScope, SourceKind, StageDefinition,
    StageRuntime, StageSource, StoredDefinition, StoredSentinel, TargetIdentity, TransitionRecord,
    MAX_SENTINEL_BATCH, MAX_SENTINEL_LINEAGE, MAX_SENTINEL_RECORDS, OTHER_BUCKET,
};

impl Storage {
    pub fn sentinels(&self) -> SentinelRepository {
        SentinelRepository {
            storage: self.clone(),
        }
    }
}

/// Bounded typed Sentinel reads and revision-fenced mutations over the shared owner.
#[derive(Clone)]
pub struct SentinelRepository {
    storage: Storage,
}

type Mutation = Completion<Committed<SentinelCommit>>;

fn refused(error: StorageError) -> Mutation {
    Completion::settled(Err(error))
}

fn batch_bound(field: &'static str, len: usize) -> Result<(), StorageError> {
    if len == 0 || len > MAX_SENTINEL_BATCH {
        return Err(StorageError::invalid(
            field,
            format!("must hold 1..={MAX_SENTINEL_BATCH} entries"),
        ));
    }
    Ok(())
}

impl SentinelRepository {
    /// Every stage in durable order, each a coherent aggregate of one committed revision.
    pub fn inventory(&self) -> Completion<Read<Vec<StoredSentinel>>> {
        self.storage.submit_read(|cx| queries::inventory(cx.conn))
    }

    /// Every stage read through the writer after all earlier admitted writes settled; the
    /// reconciliation barrier for an unconfirmed commit. It writes no rows.
    pub fn settled_inventory(&self) -> Completion<Committed<Vec<StoredSentinel>>> {
        self.storage
            .submit_mutation(overhead(), |cx| queries::inventory(cx.conn))
    }

    /// One workspace plus exact Team scope, read in a single snapshot.
    pub fn scope(
        &self,
        workspace: String,
        team: Option<String>,
    ) -> Completion<Read<Vec<StoredSentinel>>> {
        self.storage
            .submit_read(move |cx| queries::scope(cx.conn, &workspace, team.as_deref()))
    }

    pub fn workspace(&self, workspace: String) -> Completion<Read<Vec<StoredSentinel>>> {
        self.storage
            .submit_read(move |cx| queries::workspace(cx.conn, &workspace))
    }

    pub fn get(&self, id: SentinelId) -> Completion<Read<Option<StoredSentinel>>> {
        self.storage
            .submit_read(move |cx| queries::load_one(cx.conn, id.as_str()))
    }

    /// Every immutable definition version of one stage, oldest first.
    pub fn history(&self, id: SentinelId) -> Completion<Read<Vec<StoredDefinition>>> {
        self.storage
            .submit_read(move |cx| definitions::load_history(cx.conn, id.as_str()))
    }

    pub fn blueprint_log(
        &self,
        id: SentinelId,
        limit: u16,
    ) -> Completion<Read<Vec<BlueprintJournalEntry>>> {
        if !(1..=200).contains(&limit) {
            return Completion::settled(Err(crate::error::StorageError::invalid(
                "blueprint log limit",
                "must be between 1 and 200",
            )));
        }
        self.storage
            .submit_read(move |cx| pipeline::log(cx, id.as_str(), limit))
    }

    pub fn blueprint_logs(
        &self,
        ids: Vec<SentinelId>,
        limit: u16,
    ) -> Completion<Read<BTreeMap<String, Vec<BlueprintJournalEntry>>>> {
        if ids.is_empty() || ids.len() > 256 || !(1..=200).contains(&limit) {
            return Completion::settled(Err(crate::error::StorageError::invalid(
                "blueprint logs",
                "requires 1..=256 blueprints and a limit between 1 and 200",
            )));
        }
        self.storage
            .submit_read(move |cx| pipeline::logs(cx, &ids, limit))
    }

    pub fn write_blueprint_journal(
        &self,
        id: SentinelId,
        write: BlueprintJournalWrite,
    ) -> Mutation {
        if let Err(error) = write.validate() {
            return refused(error);
        }
        let cost = id.as_str().len() + serde_json::to_vec(&write).map_or(0, |bytes| bytes.len());
        self.storage
            .submit_mutation(cost, move |cx| pipeline::write(cx, id.as_str(), write))
    }

    pub fn create(&self, create: SentinelCreate) -> Mutation {
        if let Err(error) = create.validate().and_then(|()| {
            if create.scenario_id.is_none() && !create.associate.is_empty() {
                return Err(StorageError::invalid(
                    "scenario associations",
                    "require a scenario",
                ));
            }
            Ok(())
        }) {
            return refused(error);
        }
        self.storage
            .submit_mutation(create.cost(), move |cx| runtime::create(cx, create))
    }

    /// Inserts exact existing stages as `Imported` version 1; nothing is executed or rearmed.
    pub fn import(&self, records: Vec<SentinelImport>) -> Mutation {
        let validated = batch_bound("blueprint import", records.len()).and_then(|()| {
            let mut ids = HashSet::new();
            for record in &records {
                record.validate()?;
                if !ids.insert(record.identity.id.as_str()) {
                    return Err(StorageError::invalid(
                        "blueprint import",
                        "repeats a stage id",
                    ));
                }
            }
            Ok(())
        });
        if let Err(error) = validated {
            return refused(error);
        }
        let cost = records.iter().map(SentinelImport::cost).sum();
        self.storage
            .submit_mutation(cost, move |cx| runtime::import(cx, records))
    }

    /// Appends a new immutable version against the expected current head version and
    /// advances the stage generation in the same commit.
    pub fn update_definition(
        &self,
        id: SentinelId,
        expect_version: u64,
        update: DefinitionUpdate,
    ) -> Mutation {
        if let Err(error) = update.validate() {
            return refused(error);
        }
        let cost = overhead() + id.as_str().len() + update.cost();
        self.storage.submit_mutation(cost, move |cx| {
            runtime::update_definition(cx, &id, expect_version, update)
        })
    }

    pub fn set_run_binding(
        &self,
        id: SentinelId,
        expected_generation: u64,
        cron_id: String,
        expected_run: Option<RunRecord>,
        run: Option<RunRecord>,
        next_run: u64,
        last_reason: Option<String>,
    ) -> Mutation {
        let validate_run = |run: &RunRecord| {
            records::identifier("run id", &run.run_id)
                .and_then(|()| records::identifier("run tab id", &run.tab_id))
                .and_then(|()| to_sql_u64("run bound time", run.bound_at).map(|_| ()))
        };
        let validated = records::identifier("cron id", &cron_id)
            .and_then(|()| to_sql_u64("run generation", expected_generation).map(|_| ()))
            .and_then(|()| to_sql_u64("next run", next_run).map(|_| ()))
            .and_then(|()| expected_run.as_ref().map_or(Ok(()), validate_run))
            .and_then(|()| run.as_ref().map_or(Ok(()), validate_run))
            .and_then(|()| {
                last_reason
                    .as_deref()
                    .map_or(Ok(()), |reason| records::text("run reason", reason))
            });
        if let Err(error) = validated {
            return refused(error);
        }
        let cost = id.as_str().len()
            + cron_id.len()
            + expected_run
                .as_ref()
                .map_or(0, |run| run.run_id.len() + run.tab_id.len())
            + run
                .as_ref()
                .map_or(0, |run| run.run_id.len() + run.tab_id.len())
            + last_reason.as_ref().map_or(0, String::len);
        self.storage.submit_mutation(cost, move |cx| {
            runtime::set_run_binding(
                cx,
                &id,
                expected_generation,
                &cron_id,
                expected_run.as_ref(),
                run.as_ref(),
                next_run,
                last_reason.as_deref(),
            )
        })
    }

    /// Unconditionally stops and advances the generation, fencing every older lease.
    pub fn stop(&self, id: SentinelId, reason: String) -> Mutation {
        if let Err(error) = text("stop reason", &reason) {
            return refused(error);
        }
        let cost = overhead() + id.as_str().len() + reason.len();
        self.storage
            .submit_mutation(cost, move |cx| runtime::stop(cx, &id, &reason))
    }

    pub fn remove_team(&self, team: String) -> Mutation {
        if let Err(error) = identifier("team id", &team) {
            return refused(error);
        }
        self.storage
            .submit_mutation(overhead() + team.len(), move |cx| {
                runtime::remove_team(cx, &team)
            })
    }

    pub fn delete_scenario(
        &self,
        id: SentinelId,
        workspace: String,
        team: Option<String>,
    ) -> Mutation {
        let validated = identifier("workspace id", &workspace).and_then(|()| {
            team.as_deref()
                .map_or(Ok(()), |team| identifier("team id", team))
        });
        if let Err(error) = validated {
            return refused(error);
        }
        let cost =
            overhead() + id.as_str().len() + workspace.len() + team.as_ref().map_or(0, String::len);
        self.storage.submit_mutation(cost, move |cx| {
            runtime::delete_scenario(cx, id, &workspace, team.as_deref())
        })
    }

    pub fn schedule(&self, updates: Vec<ScheduleUpdate>) -> Mutation {
        let validated = batch_bound("blueprint schedule", updates.len())
            .and_then(|()| updates.iter().try_for_each(ScheduleUpdate::validate));
        if let Err(error) = validated {
            return refused(error);
        }
        let cost = updates.iter().map(ScheduleUpdate::cost).sum();
        self.storage
            .submit_mutation(cost, move |cx| runtime::schedule(cx, updates))
    }

    /// Records a lease's non-transition outcome while its generation is still current.
    pub fn finish(&self, id: SentinelId, generation: u64, outcome: RuntimeOutcome) -> Mutation {
        if let Err(error) = outcome.validate() {
            return refused(error);
        }
        let cost = id.as_str().len() + outcome.cost();
        self.storage.submit_mutation(cost, move |cx| {
            runtime::finish(cx, &id, generation, &outcome)
        })
    }

    pub fn accept_transition(
        &self,
        id: SentinelId,
        generation: u64,
        outcome: RuntimeOutcome,
        transition: NewTransition,
    ) -> Mutation {
        if let Err(error) = outcome.validate().and_then(|()| transition.validate()) {
            return refused(error);
        }
        let cost = id.as_str().len() + outcome.cost() + transition.cost();
        self.storage.submit_mutation(cost, move |cx| {
            runtime::accept_transition(cx, &id, generation, &outcome, &transition)
        })
    }

    /// Durable claim-before-action: commits an unconfirmed claim or writes nothing.
    pub fn claim_handler(
        &self,
        id: SentinelId,
        fence: BatchFence,
        claim: HandlerResultRecord,
    ) -> Mutation {
        let validated = fence
            .validate()
            .and_then(|()| claim.validate())
            .and_then(|()| {
                if claim.status != HandlerStatus::Unconfirmed
                    || claim.execution_id.is_some()
                    || claim.command_result.is_some()
                {
                    return Err(StorageError::invalid(
                        "handler claim",
                        "must be an unconfirmed claim without an effect",
                    ));
                }
                Ok(())
            });
        if let Err(error) = validated {
            return refused(error);
        }
        let cost = id.as_str().len() + fence.cost() + claim.cost();
        self.storage.submit_mutation(cost, move |cx| {
            runtime::claim_handler(cx, &id, &fence, &claim)
        })
    }

    pub fn finish_handler_result(
        &self,
        id: SentinelId,
        transition: String,
        result: HandlerResultRecord,
    ) -> Mutation {
        let validated = identifier("transition id", &transition)
            .and_then(|()| result.validate())
            .and_then(|()| {
                if result.claim_id.is_none() {
                    return Err(StorageError::invalid(
                        "handler result",
                        "must name its durable claim",
                    ));
                }
                Ok(())
            });
        if let Err(error) = validated {
            return refused(error);
        }
        let cost = id.as_str().len() + transition.len() + result.cost();
        self.storage.submit_mutation(cost, move |cx| {
            runtime::finish_handler_result(cx, &id, &transition, &result)
        })
    }

    pub fn skip_handlers(
        &self,
        id: SentinelId,
        transition: String,
        skipped: Vec<HandlerResultRecord>,
    ) -> Mutation {
        let validated = identifier("transition id", &transition)
            .and_then(|()| batch_bound("skipped handlers", skipped.len()))
            .and_then(|()| {
                skipped.iter().try_for_each(|record| {
                    record.validate()?;
                    if record.status != HandlerStatus::Skipped {
                        return Err(StorageError::invalid(
                            "skipped handler",
                            "must carry the skipped status",
                        ));
                    }
                    Ok(())
                })
            });
        if let Err(error) = validated {
            return refused(error);
        }
        let cost = id.as_str().len()
            + transition.len()
            + skipped.iter().map(HandlerResultRecord::cost).sum::<usize>();
        self.storage.submit_mutation(cost, move |cx| {
            runtime::skip_handlers(cx, &id, &transition, &skipped)
        })
    }

    /// Unpauses `target` once for a live source holding a declared unconfirmed claim.
    pub fn activate_target(
        &self,
        source: SentinelId,
        fence: BatchFence,
        target: SentinelId,
        active_limit: usize,
        input_due_at: Option<u64>,
    ) -> Mutation {
        if let Err(error) = fence.validate() {
            return refused(error);
        }
        let cost = source.as_str().len() + fence.cost() + target.as_str().len();
        self.storage.submit_mutation(cost, move |cx| {
            runtime::activate_target(cx, &source, &fence, &target, active_limit, input_due_at)
        })
    }
}
