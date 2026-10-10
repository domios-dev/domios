use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use super::user_sends::{
    atomic_replace, digest, owner_directory, read_bounded, sync_directory, write_new,
    write_replace, MAX_PREPARED_BYTES, MAX_UNSETTLED,
};

const MAX_INDEX_BYTES: u64 = 256 * 1024;
const MAX_DIRECTORY_ENTRIES: usize = MAX_UNSETTLED * 2 + 4;
const MAX_RECEIPT_BYTES: u64 = MAX_PREPARED_BYTES as u64 * 2 + 16 * 1024;
const MAX_PENDING_RECEIPTS: usize = 1024;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentDeliveryState {
    Pending,
    Claimed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct AgentDeliveryRecord {
    pub(crate) id: String,
    pub(crate) sequence: u64,
    pub(crate) state: AgentDeliveryState,
    pub(crate) idempotency_key: Option<String>,
    owner_epoch: String,
    digest: String,
    bytes: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct AgentDeliveryRecovery {
    pub(crate) payload_kind: String,
    pub(crate) worker_brief: bool,
    pub(crate) report: bool,
    pub(crate) observer: Option<super::super::messenger::DeliveryWriteObserver>,
    pub(crate) exact_text: String,
    pub(crate) anchor: Option<super::super::chat_send_transaction::SendEvidenceAnchor>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentDeliveryReceiptStatus {
    Delivered,
    SubmitUnconfirmed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct AgentDeliveryReceipt {
    pub(crate) id: String,
    pub(crate) sequence: u64,
    pub(crate) tab_id: String,
    pub(crate) payload_kind: String,
    pub(crate) worker_brief: bool,
    pub(crate) report: bool,
    pub(crate) observer: Option<super::super::messenger::DeliveryWriteObserver>,
    pub(crate) exact_text: String,
    pub(crate) anchor: Option<super::super::chat_send_transaction::SendEvidenceAnchor>,
    pub(crate) status: Option<AgentDeliveryReceiptStatus>,
    pub(crate) settlement_notified: bool,
}

#[derive(Clone, Deserialize, Serialize)]
struct AgentDeliveryIndex {
    version: u8,
    next_sequence: u64,
    records: Vec<AgentDeliveryRecord>,
}

impl Default for AgentDeliveryIndex {
    fn default() -> Self {
        Self {
            version: 2,
            next_sequence: 1,
            records: Vec::new(),
        }
    }
}

struct AgentDeliveryTab {
    tab_id: String,
    dir: PathBuf,
    index: Mutex<Option<AgentDeliveryIndex>>,
    running: AtomicBool,
    blocked_on_refusal: AtomicBool,
    state_generation: AtomicU64,
}

pub(crate) struct AgentDeliveryStore {
    root: PathBuf,
    epoch: String,
    root_initialized: Mutex<bool>,
    tabs: Mutex<HashMap<String, Arc<AgentDeliveryTab>>>,
}

impl AgentDeliveryStore {
    pub(crate) fn open(data_dir: &Path) -> Self {
        Self {
            root: data_dir.join("agent-delivery-outbox"),
            epoch: uuid::Uuid::new_v4().to_string(),
            root_initialized: Mutex::new(false),
            tabs: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn enqueue(
        &self,
        tab_id: &str,
        payload: &str,
        idempotency_key: Option<String>,
    ) -> Result<String, String> {
        if payload.len() > MAX_PREPARED_BYTES {
            return Err("agent delivery payload exceeds 4 MiB".into());
        }
        let tab = self.tab(tab_id);
        let mut index = tab.index.lock();
        let index = self.load(&tab, &mut index)?;
        if let Some(key) = idempotency_key.as_deref() {
            if let Some(existing) = index
                .records
                .iter()
                .find(|record| record.idempotency_key.as_deref() == Some(key))
            {
                return Ok(existing.id.clone());
            }
        }
        let aggregate = index
            .records
            .iter()
            .try_fold(payload.len(), |total, record| {
                total.checked_add(record.bytes)
            })
            .ok_or("agent delivery payload accounting overflow")?;
        if index.records.len() >= MAX_UNSETTLED || aggregate > MAX_PREPARED_BYTES {
            return Err("agent delivery capacity: 32 unsettled / 4 MiB per tab".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let record = AgentDeliveryRecord {
            id: id.clone(),
            sequence: index.next_sequence,
            state: AgentDeliveryState::Pending,
            idempotency_key,
            owner_epoch: self.epoch.clone(),
            digest: digest(payload.as_bytes()),
            bytes: payload.len(),
        };
        let mut next = index.clone();
        next.next_sequence = next_sequence(next.next_sequence)?;
        next.records.push(record);
        next.records.sort_by_key(|record| record.sequence);
        self.validate(&next)?;
        write_new(&payload_path(&tab, &id)?, payload.as_bytes())?;
        if let Err(error) = self.persist(&tab, &next) {
            let _ = fs::remove_file(payload_path(&tab, &id)?);
            return Err(error);
        }
        *index = next;
        Ok(id)
    }

    pub(crate) fn has_pending(&self, tab_id: &str) -> Result<bool, String> {
        Ok(self.records(tab_id)?.iter().any(|record| {
            matches!(
                record.state,
                AgentDeliveryState::Pending | AgentDeliveryState::Claimed
            )
        }))
    }

    pub(crate) fn pending_head(&self, tab_id: &str) -> Result<Option<AgentDeliveryRecord>, String> {
        Ok(self
            .records(tab_id)?
            .into_iter()
            .next()
            .filter(|record| record.state == AgentDeliveryState::Pending))
    }

    pub(crate) fn payload(
        &self,
        tab_id: &str,
        record: &AgentDeliveryRecord,
    ) -> Result<String, String> {
        let tab = self.tab(tab_id);
        let bytes = read_bounded(&payload_path(&tab, &record.id)?, MAX_PREPARED_BYTES as u64)?;
        if bytes.len() != record.bytes || digest(&bytes) != record.digest {
            return Err("agent delivery payload integrity failed".into());
        }
        String::from_utf8(bytes).map_err(|_| "agent delivery payload is not UTF-8".into())
    }

    #[cfg(test)]
    pub(crate) fn claim(&self, tab_id: &str, record_id: &str) -> Result<bool, String> {
        self.claim_with_recovery(
            tab_id,
            record_id,
            AgentDeliveryRecovery {
                payload_kind: "unknown".into(),
                worker_brief: false,
                report: false,
                observer: None,
                exact_text: String::new(),
                anchor: None,
            },
        )
    }

    pub(crate) fn claim_with_recovery(
        &self,
        tab_id: &str,
        record_id: &str,
        recovery: AgentDeliveryRecovery,
    ) -> Result<bool, String> {
        let tab = self.tab(tab_id);
        let mut loaded = tab.index.lock();
        let index = self.load(&tab, &mut loaded)?;
        let Some(position) = index
            .records
            .iter()
            .position(|record| record.id == record_id)
        else {
            return Ok(false);
        };
        let record = &index.records[position];
        if record.state != AgentDeliveryState::Pending || record.owner_epoch != self.epoch {
            return Ok(false);
        }
        let mut next = index.clone();
        next.records[position].state = AgentDeliveryState::Claimed;
        let recovery_path = claim_recovery_path(&tab, record_id)?;
        let recovery_dir = recovery_path
            .parent()
            .ok_or("agent delivery recovery directory missing")?;
        owner_directory(recovery_dir)?;
        let bytes = serde_json::to_vec(&recovery)
            .map_err(|_| "agent delivery recovery serialization failed")?;
        if bytes.len() as u64 > MAX_RECEIPT_BYTES {
            return Err("agent delivery recovery size bound exceeded".into());
        }
        write_new(&recovery_path, &bytes)?;
        sync_directory(recovery_dir)?;
        if let Err(error) = self.persist(&tab, &next) {
            *loaded = Some(next);
            return Err(error);
        }
        *loaded = Some(next);
        Ok(true)
    }

    pub(crate) fn release_zero_write(&self, tab_id: &str, record_id: &str) -> Result<(), String> {
        self.update(tab_id, record_id, |record| {
            if record.state != AgentDeliveryState::Claimed {
                return Err("agent delivery claim is not active".into());
            }
            record.state = AgentDeliveryState::Pending;
            Ok(true)
        })?;
        remove_if_present(&claim_recovery_path(&self.tab(tab_id), record_id)?)?;
        Ok(())
    }

    pub(crate) fn pending_receipts(
        &self,
        tab_id: &str,
    ) -> Result<Vec<AgentDeliveryReceipt>, String> {
        let tab = self.tab(tab_id);
        let mut loaded = tab.index.lock();
        self.load(&tab, &mut loaded)?;
        drop(loaded);
        let dir = pending_receipts_dir(&tab);
        owner_directory(dir.parent().ok_or("agent delivery receipt root missing")?)?;
        owner_directory(&dir)?;
        let entries = fs::read_dir(&dir)
            .map_err(|error| format!("agent delivery persistence: {error}"))?
            .take(MAX_PENDING_RECEIPTS + 1)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("agent delivery persistence: {error}"))?;
        if entries.len() > MAX_PENDING_RECEIPTS {
            return Err("agent delivery receipt work bound exceeded".into());
        }
        let mut receipts = Vec::with_capacity(entries.len());
        for entry in entries {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(id) = name.strip_suffix(".json") else {
                continue;
            };
            if uuid::Uuid::parse_str(id).is_err() {
                continue;
            }
            let receipt = serde_json::from_slice(&read_bounded(&entry.path(), MAX_RECEIPT_BYTES)?)
                .map_err(|_| "agent delivery receipt is invalid")?;
            receipts.push(receipt);
        }
        receipts.sort_by(|left: &AgentDeliveryReceipt, right| left.id.cmp(&right.id));
        Ok(receipts)
    }

    pub(crate) fn save_pending_receipt(
        &self,
        receipt: &AgentDeliveryReceipt,
    ) -> Result<(), String> {
        if uuid::Uuid::parse_str(&receipt.id).is_err() {
            return Err("invalid agent delivery identity".into());
        }
        let tab = self.tab(&receipt.tab_id);
        let mut loaded = tab.index.lock();
        self.load(&tab, &mut loaded)?;
        drop(loaded);
        persist_receipt(&pending_receipt_path(&tab, &receipt.id)?, receipt)
    }

    pub(crate) fn complete_receipt(
        &self,
        receipt: &mut AgentDeliveryReceipt,
    ) -> Result<(), String> {
        if uuid::Uuid::parse_str(&receipt.id).is_err() {
            return Err("invalid agent delivery identity".into());
        }
        let tab = self.tab(&receipt.tab_id);
        let mut loaded = tab.index.lock();
        self.load(&tab, &mut loaded)?;
        drop(loaded);
        receipt.exact_text.clear();
        receipt.anchor = None;
        persist_receipt(&settled_receipt_path(&tab, &receipt.id)?, receipt)?;
        remove_if_present(&pending_receipt_path(&tab, &receipt.id)?)?;
        sync_directory(&settled_receipts_dir(&tab))?;
        sync_directory(&pending_receipts_dir(&tab))
    }

    pub(crate) fn complete_written(&self, tab_id: &str, record_id: &str) -> Result<bool, String> {
        self.remove_record(tab_id, record_id, true)
    }

    pub(crate) fn discard_expired(&self, tab_id: &str, record_id: &str) -> Result<bool, String> {
        self.remove_record(tab_id, record_id, false)
    }

    fn remove_record(
        &self,
        tab_id: &str,
        record_id: &str,
        require_claimed: bool,
    ) -> Result<bool, String> {
        let tab = self.tab(tab_id);
        let mut loaded = tab.index.lock();
        let index = self.load(&tab, &mut loaded)?;
        if !index.records.iter().any(|record| record.id == record_id)
            || require_claimed
                && !index.records.iter().any(|record| {
                    record.id == record_id && record.state == AgentDeliveryState::Claimed
                })
        {
            return Ok(false);
        }
        let mut next = index.clone();
        next.records.retain(|record| record.id != record_id);
        if let Err(error) = self.persist(&tab, &next) {
            if require_claimed {
                *loaded = Some(next);
            }
            return Err(error);
        }
        *loaded = Some(next);
        match fs::remove_file(payload_path(&tab, record_id)?) {
            Ok(()) => sync_directory(&tab.dir)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => log::warn!("[agent-delivery] completed payload cleanup failed: {error}"),
        }
        if let Err(error) = remove_if_present(&claim_recovery_path(&tab, record_id)?) {
            log::warn!("[agent-delivery] claim evidence cleanup failed: {error}");
        }
        Ok(true)
    }

    pub(crate) fn start_worker(&self, tab_id: &str) -> bool {
        let tab = self.tab(tab_id);
        !tab.blocked_on_refusal.load(Ordering::Acquire)
            && tab
                .running
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }

    pub(crate) fn finish_worker(&self, tab_id: &str) {
        self.tab(tab_id).running.store(false, Ordering::Release);
    }

    pub(crate) fn state_generation(&self, tab_id: &str) -> u64 {
        self.tab(tab_id).state_generation.load(Ordering::Acquire)
    }

    pub(crate) fn block_after_refusal(&self, tab_id: &str, observed_generation: u64) -> bool {
        let tab = self.tab(tab_id);
        if tab.state_generation.load(Ordering::Acquire) != observed_generation {
            return false;
        }
        tab.blocked_on_refusal.store(true, Ordering::Release);
        if tab.state_generation.load(Ordering::Acquire) != observed_generation {
            tab.blocked_on_refusal.store(false, Ordering::Release);
            false
        } else {
            true
        }
    }

    pub(crate) fn state_changed(&self, tab_id: &str) {
        let tab = self.tab(tab_id);
        tab.state_generation.fetch_add(1, Ordering::AcqRel);
        tab.blocked_on_refusal.store(false, Ordering::Release);
    }

    fn records(&self, tab_id: &str) -> Result<Vec<AgentDeliveryRecord>, String> {
        let tab = self.tab(tab_id);
        let mut index = tab.index.lock();
        Ok(self.load(&tab, &mut index)?.records.clone())
    }

    fn update(
        &self,
        tab_id: &str,
        record_id: &str,
        change: impl FnOnce(&mut AgentDeliveryRecord) -> Result<bool, String>,
    ) -> Result<bool, String> {
        let tab = self.tab(tab_id);
        let mut loaded = tab.index.lock();
        let index = self.load(&tab, &mut loaded)?;
        let Some(position) = index
            .records
            .iter()
            .position(|record| record.id == record_id)
        else {
            return Ok(false);
        };
        let mut next = index.clone();
        if !change(&mut next.records[position])? {
            return Ok(false);
        }
        self.persist(&tab, &next)?;
        *loaded = Some(next);
        Ok(true)
    }

    fn tab(&self, tab_id: &str) -> Arc<AgentDeliveryTab> {
        self.tabs
            .lock()
            .entry(tab_id.to_string())
            .or_insert_with(|| {
                Arc::new(AgentDeliveryTab {
                    tab_id: tab_id.to_string(),
                    dir: self.root.join(digest(tab_id.as_bytes())),
                    index: Mutex::new(None),
                    running: AtomicBool::new(false),
                    blocked_on_refusal: AtomicBool::new(false),
                    state_generation: AtomicU64::new(0),
                })
            })
            .clone()
    }

    fn load<'a>(
        &self,
        tab: &AgentDeliveryTab,
        loaded: &'a mut Option<AgentDeliveryIndex>,
    ) -> Result<&'a mut AgentDeliveryIndex, String> {
        if loaded.is_none() {
            let mut initialized = self.root_initialized.lock();
            if !*initialized {
                owner_directory(&self.root)?;
                *initialized = true;
            }
            drop(initialized);
            owner_directory(&tab.dir)?;
            let path = tab.dir.join("index.json");
            let index = match fs::symlink_metadata(&path) {
                Ok(_) => serde_json::from_slice(&read_bounded(&path, MAX_INDEX_BYTES)?)
                    .map_err(|_| "agent delivery index is invalid")?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    AgentDeliveryIndex::default()
                }
                Err(error) => return Err(format!("agent delivery persistence: {error}")),
            };
            self.validate(&index)?;
            let mut index = index;
            let retired = index
                .records
                .iter()
                .filter(|record| record.state == AgentDeliveryState::Claimed)
                .cloned()
                .collect::<Vec<_>>();
            let pending_rebind = index.records.iter().any(|record| {
                record.state == AgentDeliveryState::Pending && record.owner_epoch != self.epoch
            });
            if !retired.is_empty() || pending_rebind {
                for record in &retired {
                    persist_retired_receipt(tab, record)?;
                }
                index
                    .records
                    .retain(|record| record.state == AgentDeliveryState::Pending);
                for record in &mut index.records {
                    record.owner_epoch = self.epoch.clone();
                }
                self.persist(tab, &index)?;
                for record in retired {
                    remove_if_present(&payload_path(tab, &record.id)?)?;
                    remove_if_present(&claim_recovery_path(tab, &record.id)?)?;
                }
            }
            *loaded = Some(index);
            self.collect_orphans(tab, loaded.as_ref().ok_or("agent delivery index missing")?)?;
        }
        loaded
            .as_mut()
            .ok_or_else(|| "agent delivery index missing".into())
    }

    fn validate(&self, index: &AgentDeliveryIndex) -> Result<(), String> {
        let encoded =
            serde_json::to_vec(index).map_err(|_| "agent delivery index serialization failed")?;
        if index.version != 2
            || index.next_sequence == 0
            || encoded.len() as u64 > MAX_INDEX_BYTES
            || index.records.len() > MAX_UNSETTLED
        {
            return Err("agent delivery index capacity or version is invalid".into());
        }
        let mut ids = HashSet::new();
        let mut sequences = HashSet::new();
        let mut bytes = 0usize;
        for record in &index.records {
            if uuid::Uuid::parse_str(&record.id).is_err()
                || uuid::Uuid::parse_str(&record.owner_epoch).is_err()
                || record.sequence == 0
                || record.sequence >= index.next_sequence
                || record.digest.len() != 64
                || !ids.insert(&record.id)
                || !sequences.insert(record.sequence)
            {
                return Err("agent delivery identity is invalid".into());
            }
            bytes = bytes
                .checked_add(record.bytes)
                .ok_or("agent delivery byte accounting overflow")?;
        }
        if index
            .records
            .windows(2)
            .any(|pair| pair[0].sequence >= pair[1].sequence)
            || bytes > MAX_PREPARED_BYTES
        {
            return Err("agent delivery ordering or byte bound is invalid".into());
        }
        Ok(())
    }

    fn collect_orphans(
        &self,
        tab: &AgentDeliveryTab,
        index: &AgentDeliveryIndex,
    ) -> Result<(), String> {
        let entries = fs::read_dir(&tab.dir)
            .map_err(|error| format!("agent delivery persistence: {error}"))?
            .take(MAX_DIRECTORY_ENTRIES + 1)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("agent delivery persistence: {error}"))?;
        if entries.len() > MAX_DIRECTORY_ENTRIES {
            return Err("agent delivery directory work bound exceeded".into());
        }
        let live: HashSet<String> = index
            .records
            .iter()
            .map(|record| format!("{}.payload", record.id))
            .collect();
        for entry in entries {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name == "index.json" || live.contains(&name) {
                continue;
            }
            if (name == "index.next.json"
                || name
                    .strip_suffix(".payload")
                    .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok()))
                && entry
                    .file_type()
                    .map_err(|error| format!("agent delivery persistence: {error}"))?
                    .is_file()
            {
                fs::remove_file(entry.path())
                    .map_err(|error| format!("agent delivery persistence: {error}"))?;
            }
        }
        collect_claim_recovery_orphans(tab, index)?;
        Ok(())
    }

    fn persist(&self, tab: &AgentDeliveryTab, index: &AgentDeliveryIndex) -> Result<(), String> {
        self.validate(index)?;
        let bytes =
            serde_json::to_vec(index).map_err(|_| "agent delivery index serialization failed")?;
        let temporary = tab.dir.join("index.next.json");
        write_replace(&temporary, &bytes)?;
        atomic_replace(&temporary, &tab.dir.join("index.json"))?;
        sync_directory(&tab.dir)
    }
}

fn payload_path(tab: &AgentDeliveryTab, id: &str) -> Result<PathBuf, String> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err("invalid agent delivery identity".into());
    }
    Ok(tab.dir.join(format!("{id}.payload")))
}

fn claim_recovery_path(tab: &AgentDeliveryTab, id: &str) -> Result<PathBuf, String> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err("invalid agent delivery identity".into());
    }
    Ok(tab.dir.join("claim-evidence").join(format!("{id}.json")))
}

fn pending_receipts_dir(tab: &AgentDeliveryTab) -> PathBuf {
    tab.dir.join("receipts").join("pending")
}

fn settled_receipts_dir(tab: &AgentDeliveryTab) -> PathBuf {
    tab.dir.join("receipts").join("settled")
}

fn pending_receipt_path(tab: &AgentDeliveryTab, id: &str) -> Result<PathBuf, String> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err("invalid agent delivery identity".into());
    }
    Ok(pending_receipts_dir(tab).join(format!("{id}.json")))
}

fn settled_receipt_path(tab: &AgentDeliveryTab, id: &str) -> Result<PathBuf, String> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err("invalid agent delivery identity".into());
    }
    Ok(settled_receipts_dir(tab).join(format!("{id}.json")))
}

fn persist_retired_receipt(
    tab: &AgentDeliveryTab,
    record: &AgentDeliveryRecord,
) -> Result<(), String> {
    let path = pending_receipt_path(tab, &record.id)?;
    if path.exists() {
        let existing: AgentDeliveryReceipt =
            serde_json::from_slice(&read_bounded(&path, MAX_RECEIPT_BYTES)?)
                .map_err(|_| "agent delivery receipt is invalid")?;
        if existing.id != record.id || existing.tab_id != tab.tab_id {
            return Err("agent delivery receipt identity is invalid".into());
        }
        return Ok(());
    }
    let recovery = read_recovery_or_unknown(tab, record);
    let receipt = AgentDeliveryReceipt {
        id: record.id.clone(),
        sequence: record.sequence,
        tab_id: tab.tab_id.clone(),
        payload_kind: recovery.payload_kind,
        worker_brief: recovery.worker_brief,
        report: recovery.report,
        observer: recovery.observer,
        exact_text: recovery.exact_text,
        anchor: recovery.anchor,
        status: None,
        settlement_notified: false,
    };
    persist_receipt(&path, &receipt)
}

fn read_recovery_or_unknown(
    tab: &AgentDeliveryTab,
    record: &AgentDeliveryRecord,
) -> AgentDeliveryRecovery {
    let recovery = claim_recovery_path(tab, &record.id)
        .ok()
        .and_then(|path| read_bounded(&path, MAX_RECEIPT_BYTES).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    if let Some(recovery) = recovery {
        return recovery;
    }
    let payload = payload_path(tab, &record.id)
        .ok()
        .and_then(|path| read_bounded(&path, MAX_PREPARED_BYTES as u64).ok())
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let observer_value = payload
        .as_ref()
        .and_then(|payload| payload.get("observer"))
        .cloned();
    let observer_kind = observer_value
        .as_ref()
        .and_then(|observer| observer.get("kind"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let observer = observer_value.and_then(|value| serde_json::from_value(value).ok());
    let payload_kind = match observer_kind.as_deref() {
        Some("report") => "report",
        Some("plan_role") => "plan_role",
        Some("plan_reminder") => "reminder",
        _ => "agent_message",
    };
    AgentDeliveryRecovery {
        payload_kind: payload_kind.into(),
        worker_brief: false,
        report: observer_kind.as_deref() == Some("report"),
        observer,
        exact_text: String::new(),
        anchor: None,
    }
}

fn persist_receipt(path: &Path, receipt: &AgentDeliveryReceipt) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("agent delivery receipt directory missing")?;
    owner_directory(
        parent
            .parent()
            .ok_or("agent delivery receipt root missing")?,
    )?;
    owner_directory(parent)?;
    let bytes =
        serde_json::to_vec(receipt).map_err(|_| "agent delivery receipt serialization failed")?;
    if bytes.len() as u64 > MAX_RECEIPT_BYTES {
        return Err("agent delivery receipt size bound exceeded".into());
    }
    let temporary = path.with_extension("next.json");
    write_replace(&temporary, &bytes)?;
    atomic_replace(&temporary, path)?;
    sync_directory(parent)
}

fn remove_if_present(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => {
            if let Some(parent) = path.parent() {
                sync_directory(parent)?;
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("agent delivery persistence: {error}")),
    }
}

fn collect_claim_recovery_orphans(
    tab: &AgentDeliveryTab,
    index: &AgentDeliveryIndex,
) -> Result<(), String> {
    let dir = tab.dir.join("claim-evidence");
    match fs::symlink_metadata(&dir) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("agent delivery persistence: {error}")),
        Ok(_) => owner_directory(&dir)?,
    }
    let entries = fs::read_dir(&dir)
        .map_err(|error| format!("agent delivery persistence: {error}"))?
        .take(MAX_UNSETTLED * 2 + 1)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("agent delivery persistence: {error}"))?;
    if entries.len() > MAX_UNSETTLED * 2 {
        return Err("agent delivery claim evidence work bound exceeded".into());
    }
    let live = index
        .records
        .iter()
        .filter(|record| record.state == AgentDeliveryState::Claimed)
        .map(|record| format!("{}.json", record.id))
        .collect::<HashSet<_>>();
    for entry in entries {
        let name = entry.file_name();
        if name.to_str().is_some_and(|name| live.contains(name)) {
            continue;
        }
        if entry
            .file_type()
            .map_err(|error| format!("agent delivery persistence: {error}"))?
            .is_file()
        {
            fs::remove_file(entry.path())
                .map_err(|error| format!("agent delivery persistence: {error}"))?;
        }
    }
    sync_directory(&dir)
}

fn next_sequence(sequence: u64) -> Result<u64, String> {
    sequence
        .checked_add(1)
        .ok_or_else(|| "agent delivery sequence exhausted".into())
}

#[cfg(test)]
#[path = "agent_deliveries/tests.rs"]
mod tests;
