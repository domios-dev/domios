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
const MAX_DIRECTORY_ENTRIES: usize = MAX_UNSETTLED * 2 + 2;

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

    pub(crate) fn claim(&self, tab_id: &str, record_id: &str) -> Result<bool, String> {
        self.update(tab_id, record_id, |record| {
            if record.state != AgentDeliveryState::Pending || record.owner_epoch != self.epoch {
                return Ok(false);
            }
            record.state = AgentDeliveryState::Claimed;
            Ok(true)
        })
    }

    pub(crate) fn release_zero_write(&self, tab_id: &str, record_id: &str) -> Result<(), String> {
        self.update(tab_id, record_id, |record| {
            if record.state != AgentDeliveryState::Claimed {
                return Err("agent delivery claim is not active".into());
            }
            record.state = AgentDeliveryState::Pending;
            Ok(true)
        })?;
        Ok(())
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
                .map(|record| record.id.clone())
                .collect::<Vec<_>>();
            let pending_rebind = index.records.iter().any(|record| {
                record.state == AgentDeliveryState::Pending && record.owner_epoch != self.epoch
            });
            if !retired.is_empty() || pending_rebind {
                index
                    .records
                    .retain(|record| record.state == AgentDeliveryState::Pending);
                for record in &mut index.records {
                    record.owner_epoch = self.epoch.clone();
                }
                self.persist(tab, &index)?;
                for id in retired {
                    match fs::remove_file(payload_path(tab, &id)?) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => return Err(format!("agent delivery persistence: {error}")),
                    }
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

fn next_sequence(sequence: u64) -> Result<u64, String> {
    sequence
        .checked_add(1)
        .ok_or_else(|| "agent delivery sequence exhausted".into())
}
