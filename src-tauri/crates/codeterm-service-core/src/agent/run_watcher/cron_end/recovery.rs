use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use super::*;

pub(super) const MAX_OWNERS: usize = 64;
pub(super) const MAX_SUMMARY: usize = 16 * 1024;
pub(super) const MAX_TAIL: usize = 8 * 1024;
const MAX_RECORD: usize = 32 * 1024;
const MAX_JOURNAL: usize = MAX_OWNERS * MAX_RECORD + 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Phase { Planned, Allocating, Provisional, FinishPending, Confirmed, Cleanup, DeliveryIntent, DeliveryKnown, Report, Retire, Unknown, Adjudicated }

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Owner {
    tab: String, nonce: Option<String>, pty: Option<String>,
    pad_nonce: Option<String>, pad_generation: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Record {
    revision: u64,
    pub(super) job: String,
    pub(super) run: String,
    owner: Option<Owner>,
    pub(super) phase: Phase,
    pub(super) unknown_at: Option<Phase>,
    proof: Option<String>,
    intent: Option<String>,
    pub(super) plan: Option<crate::service::cron::CronReportPlan>,
    pub(super) delivery: Option<String>,
    bytes: u64,
    tail_digest: String,
}

impl Record {
    fn cleanup_was_confirmed(&self) -> bool {
        let phase = if self.phase == Phase::Unknown { self.unknown_at } else { Some(self.phase) };
        self.proof.is_some() && self.intent.is_some()
            && matches!(phase, Some(Phase::DeliveryIntent | Phase::DeliveryKnown | Phase::Report | Phase::Retire))
    }

    fn acknowledged_uncaptured_allocation(&self) -> bool {
        self.phase == Phase::Adjudicated && self.unknown_at == Some(Phase::Allocating) && self.owner.is_none()
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Journal { #[serde(deserialize_with = "bounded_records")] records: Vec<Record> }

fn bounded_records<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<Record>, D::Error> {
    struct Records;
    impl<'de> serde::de::Visitor<'de> for Records {
        type Value = Vec<Record>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { formatter.write_str("a bounded cron owner list") }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
            let mut records = Vec::new();
            while let Some(record) = sequence.next_element()? {
                if records.len() == MAX_OWNERS { return Err(serde::de::Error::custom("cron owner capacity exceeded")); }
                records.push(record);
            }
            Ok(records)
        }
    }
    deserializer.deserialize_seq(Records)
}

pub(crate) struct CancellationSnapshot { records: Vec<Record> }
impl CancellationSnapshot { pub(crate) fn is_empty(&self) -> bool { self.records.is_empty() } }

#[derive(Default)]
pub(crate) struct Recovery {
    root: Option<PathBuf>,
    digest: Option<String>,
    pub(super) records: BTreeMap<(String, String), Record>,
    incident: Option<String>,
    restored: bool,
    restoring: bool,
}

pub(super) fn bounded(text: &str, limit: usize) -> String {
    if text.len() <= limit { return text.into(); }
    let mut end = limit.saturating_sub(48);
    while !text.is_char_boundary(end) { end -= 1; }
    format!("{}\n[truncated from {} bytes]", &text[..end], text.len())
}

pub(super) fn tail(text: &str) -> String {
    let mut start = text.len().saturating_sub(MAX_TAIL);
    while !text.is_char_boundary(start) { start += 1; }
    text[start..].into()
}

fn digest(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }

fn key(job: &str, run: &str) -> Result<(String, String), String> {
    let valid = |value: &str| !value.is_empty() && value.len() <= 128 && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if !valid(job) || !valid(run) { return Err("Cron recovery identifiers are invalid".into()); }
    Ok((job.into(), run.into()))
}

fn read(path: &std::path::Path) -> Result<Option<Vec<u8>>, String> {
    let file = match std::fs::File::open(path) { Ok(file) => file, Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None), Err(error) => return Err(error.to_string()) };
    let mut bytes = Vec::new();
    file.take((MAX_JOURNAL + 1) as u64).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_JOURNAL { return Err("Cron continuation journal exceeds its bound; preserve it for repair".into()); }
    Ok(Some(bytes))
}

impl Recovery {
    fn path(&self) -> Result<PathBuf, String> { Ok(self.root.as_ref().ok_or("Cron recovery is not initialized")?.join(".terminal-owners.json")) }

    fn load(&mut self, root: PathBuf) -> Result<(), String> {
        if let Some(incident) = &self.incident { return Err(incident.clone()); }
        if self.root.is_some() { return if self.root.as_ref() == Some(&root) { Ok(()) } else { Err("Cron recovery root changed".into()) }; }
        self.root = Some(root);
        let result = (|| {
            let bytes = read(&self.path()?)?;
            let journal: Journal = bytes.as_ref().map(|bytes| serde_json::from_slice(bytes).map_err(|error| format!("Cron journal is invalid; preserve for repair: {error}"))).transpose()?.unwrap_or_default();
            if journal.records.len() > MAX_OWNERS { return Err("Cron journal owner capacity is invalid; preserve for repair".into()); }
            for record in journal.records {
                let identity = key(&record.job, &record.run)?;
                if serde_json::to_vec(&record).map_err(|error| error.to_string())?.len() > MAX_RECORD || self.records.contains_key(&identity) { return Err("Cron journal record exceeds its bound or repeats an identity".into()); }
                if let Some(owner) = &record.owner {
                    key(&owner.tab, &record.run)?;
                    if owner.nonce.as_ref().is_some_and(|nonce| nonce.is_empty() || nonce.len() > 512)
                        || owner.pty.as_ref().is_some_and(|pty| key(pty, &record.run).is_err())
                        || owner.pad_nonce.as_ref().is_some_and(|nonce| nonce.is_empty() || nonce.len() > 512)
                        || owner.pad_generation.as_ref().is_some_and(|generation| uuid::Uuid::parse_str(generation).is_err())
                        || owner.pad_nonce.is_some() != owner.pad_generation.is_some()
                        || owner.pad_nonce.as_ref().is_some_and(|nonce| Some(nonce) != owner.nonce.as_ref()) {
                        return Err("Cron journal lifecycle identity is invalid; preserve for repair".into());
                    }
                }
                self.records.insert(identity, record);
            }
            self.digest = bytes.map(|bytes| digest(&bytes));
            Ok(())
        })();
        if let Err(error) = &result { self.incident = Some(error.clone()); }
        result
    }

    fn save(&mut self, next: BTreeMap<(String, String), Record>) -> Result<(), String> {
        if let Some(incident) = &self.incident { return Err(incident.clone()); }
        if next.len() > MAX_OWNERS { return Err("Cron terminal owner capacity reached; resolve an existing owner with cron.cancel".into()); }
        let path = self.path()?;
        let current = read(&path)?.map(|bytes| digest(&bytes));
        if current != self.digest { return Err("Cron journal changed outside its owner; preserve it for repair".into()); }
        let records: Vec<_> = next.values().cloned().collect();
        for record in &records {
            if serde_json::to_vec(record).map_err(|error| error.to_string())?.len() > MAX_RECORD { return Err("Cron continuation metadata exceeds its bound".into()); }
        }
        let bytes = serde_json::to_string(&Journal { records }).map_err(|error| error.to_string())?;
        if bytes.len() > MAX_JOURNAL { return Err("Cron journal exceeds its byte capacity".into()); }
        crate::persisted_state::StateFile::new(&path).with_private_permissions().without_backups().save(&bytes)?;
        self.digest = Some(digest(bytes.as_bytes()));
        self.records = next;
        Ok(())
    }
}

fn receipt_proof(source: &crate::cron::store::CronRunInfo) -> Option<String> {
    if !matches!(source.status, CronRunStatus::Success | CronRunStatus::Failed)
        || source.summary.as_ref().is_some_and(|text| text.len() > MAX_SUMMARY)
        || source.session_id.as_ref().is_some_and(|text| text.len() > 1024)
        || source.cwd.as_ref().is_some_and(|text| text.len() > 4096)
        || source.tab_id.as_ref().is_some_and(|text| text.len() > 128)
        || source.provider.as_ref().is_some_and(|text| text.len() > 512)
        || source.owner_instance_id.as_ref().is_some_and(|text| text.len() > 128)
        || source.report_delivery.as_ref().is_some_and(|text| text.len() > 1024)
        || source.run_id.len() > 128 { return None; }
    let mut receipt = source.clone();
    receipt.report_delivery = None;
    serde_json::to_vec(&receipt).ok().map(|bytes| digest(&bytes))
}

fn proof(store: &CronStore, job: &str, run: &str) -> Option<String> {
    let expected = receipt_proof(store.run_receipt(job, run)?)?;
    if let Some(mirror) = store.get(job).and_then(|job| job.last_run.as_ref()).filter(|mirror| mirror.run_id == run) {
        if receipt_proof(mirror).as_deref() != Some(expected.as_str()) { return None; }
    }
    Some(expected)
}

impl super::super::RunWatcherRegistry {
    pub(crate) fn settle_unallocated_cron_claim(&self, svc: &ServiceContext, cron: &crate::CronState, job: &str, run: &str) -> Result<(), String> {
        if !self.cron_allocations.lock().remove(&(job.into(), run.into())) {
            return Err("Original unallocated claim is absent; resource capacity remains retained".into());
        }
        if self.cron_record(job, run).is_some_and(|record| record.acknowledged_uncaptured_allocation()) {
            return self.archive_cron_record(cron, job, run, true);
        }
        self.settle_cron_spawn_refusal(svc, cron, job, run, "Allocation refused before original tab capture")
    }

    pub(crate) fn settle_cron_spawn_refusal(&self, svc: &ServiceContext, cron: &crate::CronState, job: &str, run: &str, reason: &str) -> Result<(), String> {
        if let Some(owner) = self.cron_cleanup_owners(job).into_iter().find(|owner| owner.run_id() == run) {
            if let Some(handle) = self.fail_sealed_cron_launch(svc, &owner, reason) { handle.wait_for_terminal()?; }
            return Ok(());
        }
        let Some(mut record) = self.cron_record(job, run) else { return Ok(()) };
        if record.acknowledged_uncaptured_allocation() { return Err("Uncaptured original resource capacity remains reserved; cancellation acknowledgment is not cleanup proof".into()); }
        if record.owner.is_some() {
            if self.cron_finalizers.lock().contains_key(&(job.into(), run.into())) { return Ok(()); }
            return Err("Cron launch retains original generation evidence; use cron.cancel to settle its cleanup".into());
        }
        if record.phase == Phase::Adjudicated && cron.lock().run_is_cancelled(job, run) { return self.archive_cron_record(cron, job, run, true); }
        if !matches!(record.phase, Phase::Planned | Phase::Allocating) { return Err("Cron allocation refusal retains an unresolved original outcome; use cron.cancel".into()); }
        let request = TerminalRun { job_id:job.into(), run_id:run.into(), status:CronRunStatus::Failed, summary:bounded(reason, MAX_SUMMARY), session_id:None, cwd:None, tab_id:None };
        let mut attempt = TerminalRunCommit::new(request);
        let finished = match crate::async_runtime::block_on(attempt.settle(cron)) {
            Ok(finished) => finished,
            Err(error) => {
                record.unknown_at = Some(Phase::FinishPending);
                record.phase = Phase::Unknown;
                self.replace_cron_record(record)?;
                return Err(error.to_string());
            }
        };
        if finished.fresh {
            log_finished(job, run, &CronRunStatus::Failed, finished);
            crate::cron::notify::notify_on_consecutive_failure(svc, cron, job);
            record.phase = Phase::Confirmed;
            record.proof = proof(&cron.lock(), job, run);
            self.replace_cron_record(record)?;
        }
        self.archive_cron_record(cron, job, run, !finished.fresh)
    }

    pub(crate) fn ensure_cron_recovery(&self, svc: &ServiceContext, cron: &crate::CronState) -> Result<(), String> {
        let root = cron.lock().terminal_recovery_root();
        let records = {
            let mut recovery = self.cron_recovery.lock();
            recovery.load(root)?;
            if recovery.restored { return Ok(()); }
            if recovery.restoring { return Err("Cron continuation recovery is already running".into()); }
            recovery.restoring = true;
            recovery.records.values().cloned().collect::<Vec<_>>()
        };
        let result = (|| {
            for mut record in records {
                if record.acknowledged_uncaptured_allocation() {
                    svc.log_system_notice("Cron uncaptured allocation remains owned".into(), Some(format!("job={} run={}; effects were acknowledged, original tab identity was not durably captured; its global capacity remains reserved and no resource close is authorized", record.job, record.run)), Some("error"));
                    continue;
                }
                if record.phase == Phase::Unknown {
                    svc.log_system_notice("Cron continuation remains unknown".into(), Some(format!("job={} run={}; original evidence retained for exact cron.cancel adjudication without replay", record.job, record.run)), Some("error"));
                    continue;
                }
                let receipt_valid = {
                    let store = cron.lock();
                    record.proof.is_some() && record.proof == proof(&store, &record.job, &record.run)
                };
                let cancelled = cron.lock().run_is_cancelled(&record.job, &record.run);
                let mut lifecycle_valid = record.owner.is_some();
                if cancelled || receipt_valid {
                    match self.restore_journal_owner(svc, cron, &record, false)? {
                        Some(owner) => { svc.settle_owned_cron_cleanup(&owner, TabCloseReason::AgentRunEnded)?; }
                        None => { lifecycle_valid = false; }
                    }
                }
                if cancelled && record.phase == Phase::Adjudicated && lifecycle_valid {
                    self.archive_cron_record(cron, &record.job, &record.run, true)?;
                    continue;
                }
                if receipt_valid && lifecycle_valid && matches!(record.phase, Phase::DeliveryKnown | Phase::Report | Phase::Retire) && record.delivery.is_some() {
                    let receipt = cron.lock().run_receipt(&record.job, &record.run).cloned().ok_or("Cron recovery receipt disappeared")?;
                    let request = TerminalRun { job_id: record.job.clone(), run_id: record.run.clone(), status: receipt.status, summary: receipt.summary.unwrap_or_default(), session_id: receipt.session_id, cwd: receipt.cwd, tab_id: receipt.tab_id };
                    let mut attempt = TerminalRunCommit::report(request.clone(), record.delivery.clone().unwrap_or_default());
                    crate::async_runtime::block_on(attempt.settle(cron)).map_err(|error| error.to_string())?;
                    if record.plan.as_ref().is_some_and(|plan| plan.retire_oneshot) {
                        let mut retire = TerminalRunCommit::retire(request);
                        crate::async_runtime::block_on(retire.settle(cron)).map_err(|error| error.to_string())?;
                    }
                    self.archive_cron_record(cron, &record.job, &record.run, false)?;
                } else {
                    record.unknown_at.get_or_insert(record.phase);
                    record.phase = Phase::Unknown;
                    let detail = format!("job={} run={}; use cron.cancel on this exact job to acknowledge unknown effects without replay", record.job, record.run);
                    self.replace_cron_record(record)?;
                    svc.log_system_notice("Cron continuation outcome requires adjudication".into(), Some(detail), Some("error"));
                }
            }
            Ok(())
        })();
        let mut recovery = self.cron_recovery.lock();
        recovery.restoring = false;
        recovery.restored = result.is_ok();
        result
    }

    pub(crate) fn reserve_cron_slot(&self, svc: &ServiceContext, cron: &crate::CronState, job: &str, run: &str) -> Result<(), String> {
        self.ensure_cron_recovery(svc, cron)?;
        let identity = key(job, run)?;
        { let store = cron.lock(); store.log_file(job, run)?; if store.get(job).is_none() { return Err("Cron recovery job is absent".into()); } }
        let mut recovery = self.cron_recovery.lock();
        if self.cron_cancelling.lock().contains(job) { return Err("Cron cancellation owns admission; no new occurrence was reserved".into()); }
        if let Some(record) = recovery.records.get(&identity) {
            return if record.phase == Phase::Planned { Ok(()) } else { Err("The original launch already owns this occurrence; replacement allocation is refused".into()) };
        }
        if recovery.records.values().any(|record| record.job == job && !record.acknowledged_uncaptured_allocation()) { return Err("This cron job retains an earlier owner; use cron.cancel to adjudicate its exact evidence before a new occurrence".into()); }
        let mut next = recovery.records.clone();
        next.insert(identity, Record { revision:0, job: job.into(), run: run.into(), owner: None, phase: Phase::Planned, unknown_at: None, proof: None, intent: None, plan: None, delivery: None, bytes: 0, tail_digest: String::new() });
        recovery.save(next)
    }

    pub(crate) fn claim_cron_allocation(&self, job: &str, run: &str) -> Result<(), String> {
        let identity = key(job, run)?;
        let mut recovery = self.cron_recovery.lock();
        if self.cron_cancelling.lock().contains(job) { return Err("Cron cancellation owns allocation admission".into()); }
        let mut next = recovery.records.clone();
        let record = next.get_mut(&identity).ok_or("Cron allocation has no original reservation")?;
        if record.phase != Phase::Planned { return Err("Cron allocation already has an original owner".into()); }
        record.phase = Phase::Allocating;
        record.revision = record.revision.checked_add(1).ok_or("Cron allocation revision exhausted")?;
        recovery.save(next)?;
        self.cron_allocations.lock().insert(identity);
        Ok(())
    }

    pub(super) fn replace_cron_record(&self, mut record: Record) -> Result<(), String> {
        let identity = key(&record.job, &record.run)?;
        let mut recovery = self.cron_recovery.lock();
        let mut next = recovery.records.clone();
        let current = next.get(&identity).ok_or("Original cron continuation reservation is absent")?;
        if current.revision != record.revision { return Err("Cron continuation advanced under its original owner; stale phase was refused".into()); }
        record.revision = record.revision.checked_add(1).ok_or("Cron continuation revision exhausted")?;
        next.insert(identity, record);
        recovery.save(next)
    }

    pub(crate) fn journal_cron_owner(&self, owner: &CronRunAuthority) -> Result<(), String> {
        let mut record = self.cron_record(owner.job_id(), owner.run_id()).ok_or("Original cron reservation is absent")?;
        if !matches!(record.phase, Phase::Planned | Phase::Allocating | Phase::Provisional | Phase::Adjudicated) { return Ok(()); }
        let pad = self.cron_pad_lease(owner);
        if record.owner.as_ref().is_some_and(|prior| prior.tab != owner.tab_id() || prior.nonce.as_deref() != owner.nonce()) { return Err("Cron journal belongs to another original generation".into()); }
        let prior = record.owner.as_ref();
        record.owner = Some(Owner { tab: owner.tab_id().into(), nonce: owner.nonce().map(str::to_string), pty: owner.pty_id().map(str::to_string).or_else(|| prior.and_then(|prior| prior.pty.clone())), pad_nonce: pad.as_ref().map(|pad| pad.nonce().into()).or_else(|| prior.and_then(|prior| prior.pad_nonce.clone())), pad_generation: pad.as_ref().map(|pad| pad.generation().into()).or_else(|| prior.and_then(|prior| prior.pad_generation.clone())) });
        if record.phase != Phase::Adjudicated { record.phase = Phase::Provisional; }
        self.replace_cron_record(record)
    }

    pub(super) fn cron_record(&self, job: &str, run: &str) -> Option<Record> { self.cron_recovery.lock().records.get(&(job.into(), run.into())).cloned() }

    pub(super) fn journal_cron_intent(&self, owner: &CronRunAuthority, request: &TerminalRun) -> Result<(), String> {
        let mut record = self.cron_record(owner.job_id(), owner.run_id()).ok_or("Cron terminal intent has no original reservation")?;
        let encoded = serde_json::to_vec(&(request.job_id.as_str(), request.run_id.as_str(), &request.status, request.summary.as_str(), &request.session_id, &request.cwd, &request.tab_id)).map_err(|error| error.to_string())?;
        record.intent = Some(digest(&encoded));
        self.replace_cron_record(record)
    }

    pub(super) fn checkpoint_cron(&self, cron: &crate::CronState, owner: &CronRunAuthority, phase: Phase, output: (usize, &str), plan: Option<crate::service::cron::CronReportPlan>, delivery: Option<String>) -> Result<(), String> {
        let mut record = self.cron_record(owner.job_id(), owner.run_id()).ok_or("Original cron continuation reservation is absent")?;
        if record.phase == Phase::Adjudicated { return Err("Cron continuation was adjudicated".into()); }
        record.phase = phase;
        record.bytes = output.0 as u64;
        record.tail_digest = digest(output.1.as_bytes());
        if matches!(phase, Phase::Confirmed | Phase::Cleanup | Phase::DeliveryIntent | Phase::DeliveryKnown | Phase::Report | Phase::Retire) { record.proof = proof(&cron.lock(), owner.job_id(), owner.run_id()); }
        if plan.is_some() { record.plan = plan; }
        if delivery.is_some() { record.delivery = delivery; }
        self.replace_cron_record(record)
    }

    pub(super) fn archive_cron_record(&self, cron: &crate::CronState, job: &str, run: &str, adjudicated: bool) -> Result<(), String> {
        let path = cron.lock().log_file(job, run)?.with_extension(if adjudicated { "terminal-adjudicated.json" } else { "terminal-settled.json" });
        let mut recovery = self.cron_recovery.lock();
        let identity = key(job, run)?;
        let Some(mut record) = recovery.records.get(&identity).cloned() else { return Ok(()) };
        if self.cron_allocation_pending(job, run) { return Err("Original cron allocation still owns teardown; continuation remains retained".into()); }
        if adjudicated { record.unknown_at.get_or_insert(record.phase); record.phase = Phase::Adjudicated; }
        let bytes = serde_json::to_string(&record).map_err(|error| error.to_string())?;
        if !CronStore::create_log(&path, &bytes)? {
            let existing = std::fs::File::open(&path).map_err(|error| error.to_string())?;
            let mut prior = Vec::new();
            existing.take((MAX_RECORD + 1) as u64).read_to_end(&mut prior).map_err(|error| error.to_string())?;
            if prior != bytes.as_bytes() { return Err("Cron archive retains different original evidence; preserve both archive and live continuation for repair".into()); }
        }
        let mut next = recovery.records.clone();
        next.remove(&identity);
        recovery.save(next)
    }

    pub(super) fn restore_journal_owner(&self, svc: &ServiceContext, cron: &crate::CronState, record: &Record, allow_detached: bool) -> Result<Option<CronRunAuthority>, String> {
        key(&record.job, &record.run)?;
        let Some(owner) = &record.owner else { return Ok(None) };
        key(&owner.tab, &record.run)?;
        let valid = {
            let store = cron.lock();
            let receipt = store.run_receipt(&record.job, &record.run);
            let mirror = store.get(&record.job).and_then(|job| job.last_run.as_ref()).filter(|receipt| receipt.run_id == record.run);
            receipt.is_some() && [receipt, mirror].into_iter().flatten().all(|receipt| receipt.tab_id.as_deref().is_none_or(|tab| tab == owner.tab))
        };
        if !valid { return Err("Cron journal has no matching authoritative run receipt".into()); }
        let captured = CronRunAuthority { tab_id: owner.tab.clone(), job_id: record.job.clone(), run_id: record.run.clone(), binding_nonce: owner.nonce.clone(), pty_id: owner.pty.clone() };
        let current = svc.mux.read().get_tab(&owner.tab).is_some();
        if current && owner.nonce.as_ref().is_none_or(String::is_empty) { return Err("Cron journal has no original lifecycle generation; preserve it for adjudication".into()); }
        if current && !captured.matches(&svc.mux.read()) && !allow_detached { return Ok(None); }
        self.retain_cron_cleanup(&captured, None);
        if let (Some(nonce), Some(generation)) = (&owner.pad_nonce, &owner.pad_generation) {
            if let Some(pad) = svc.restore_cron_pad_lease(&owner.tab, nonce, generation)? { self.attach_cron_pad(&captured, pad)?; }
        }
        Ok(Some(captured))
    }

    pub(crate) fn unknown_cron_outcome(&self, run: &str) -> bool {
        let live = self.cron_finalizers.lock().values().find(|finalizer| finalizer.owner.run_id == run).cloned();
        if live.as_ref().is_some_and(|finalizer| finalizer.unknown.load(Ordering::Acquire)) { return true; }
        self.cron_recovery.lock().records.values().any(|record| record.run == run && (record.phase == Phase::Unknown || record.phase == Phase::DeliveryIntent && !live.as_ref().is_some_and(|finalizer| finalizer.report.lock().as_ref().is_some_and(|(_, label)| !label.is_empty()))))
    }

    pub(super) fn mark_cron_unknown(&self, owner: &CronRunAuthority) -> Result<(), String> {
        let mut record = self.cron_record(owner.job_id(), owner.run_id()).ok_or("Cron unknown outcome has no original reservation")?;
        record.unknown_at.get_or_insert(record.phase);
        record.phase = Phase::Unknown;
        self.replace_cron_record(record)
    }

    pub(crate) fn cron_cancellation_snapshot(&self, job: &str) -> CancellationSnapshot {
        CancellationSnapshot { records: self.cron_recovery.lock().records.values().filter(|record| record.job == job).cloned().collect() }
    }

    pub(crate) fn adjudicate_cron_cancellation(&self, svc: &ServiceContext, cron: &crate::CronState, snapshot: CancellationSnapshot, live: &[CronRunAuthority]) -> Result<(), String> {
        for record in snapshot.records {
            if cron.lock().run_is_active(&record.job, &record.run) { return Err("Cron cancellation has not established terminal authority for the retained occurrence; original owner remains".into()); }
            let current = self.cron_record(&record.job, &record.run).unwrap_or_else(|| record.clone());
            if cron.lock().run_receipt(&record.job, &record.run).is_none() && current.cleanup_was_confirmed() {
                self.archive_cron_record(cron, &record.job, &record.run, true)?;
                continue;
            }
            let owner = live.iter().find(|owner| owner.job_id() == record.job && owner.run_id() == record.run).cloned()
                .or_else(|| self.cron_cleanup_owners(&record.job).into_iter().find(|owner| owner.run_id() == record.run));
            if owner.is_none() && current.owner.is_none() && current.unknown_at == Some(Phase::Allocating) {
                let mut acknowledged = current;
                acknowledged.phase = Phase::Adjudicated;
                self.replace_cron_record(acknowledged)?;
                svc.log_system_notice("Cron unknown effects acknowledged; allocation capacity retained".into(), Some(format!("job={} run={}; original tab identity was not durably captured; no resource was closed, evidence and one global capacity slot remain owned for exact resource adjudication", record.job, record.run)), Some("error"));
                continue;
            }
            let owner = match owner { Some(owner) => Some(owner), None => self.restore_journal_owner(svc, cron, &current, true)? };
            if let Some(owner) = &owner {
                self.cancel_cron_finalizer(owner);
                if let Err(error) = cleanup(svc, cron, owner, TabCloseReason::Cancelled) {
                    if !self.cron_allocation_pending(&record.job, &record.run) { return Err(error); }
                }
            }
            if self.cron_allocation_pending(&record.job, &record.run) {
                let mut pending = self.cron_record(&record.job, &record.run).ok_or("Cancelled allocation lost its original continuation")?;
                pending.unknown_at.get_or_insert(pending.phase);
                pending.phase = Phase::Adjudicated;
                self.replace_cron_record(pending)?;
                svc.log_system_notice("Cancelled cron allocation cleanup remains owned".into(), Some(format!("job={} run={}; original construction release will resume exact-generation teardown", record.job, record.run)), Some("error"));
                continue;
            }
            self.archive_cron_record(cron, &record.job, &record.run, true)?;
            if let Some(owner) = owner { self.forget_adjudicated_finalizer(&owner); }
        }
        Ok(())
    }

    pub(crate) fn cron_allocation_pending(&self, job: &str, run: &str) -> bool {
        self.cron_allocations.lock().contains(&(job.into(), run.into()))
    }

    pub(crate) fn release_cron_allocation_reservation(&self, job: &str, run: &str) {
        self.cron_allocations.lock().remove(&(job.into(), run.into()));
    }

    pub(crate) fn release_cron_allocation(&self, svc: &ServiceContext, owner: &CronRunAuthority) {
        self.release_cron_allocation_reservation(owner.job_id(), owner.run_id());
        let adjudicated = self.cron_record(owner.job_id(), owner.run_id()).is_some_and(|record| record.phase == Phase::Adjudicated);
        let cancelled = svc.try_ext::<crate::CronState>().is_some_and(|cron| cron.lock().run_is_cancelled(owner.job_id(), owner.run_id()));
        let retained = self.cron_finalizers.lock().contains_key(&(owner.job_id().into(), owner.run_id().into()));
        if retained || adjudicated && cancelled {
            self.resume_allocation_finalizer(svc, owner);
        }
    }

    pub(super) fn retained_cron_jobs(&self, job: &str) -> bool { self.cron_recovery.lock().records.values().any(|record| record.job == job && record.phase != Phase::Planned && !record.acknowledged_uncaptured_allocation()) }
}
