use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

mod admission_evidence;
mod admission_tokens;
pub(crate) use admission_evidence::UserAdmissionEvidence;
pub(crate) mod continuity;
mod ready_launch;
mod reservations;
pub(crate) mod runtime_questions;
use admission_tokens::{AdmissionTokens, TokenEntry};
use ready_launch::ReadyLaunchLatch;

pub(crate) const MAX_UNSETTLED: usize = 32;
pub(crate) const MAX_PREPARED_BYTES: usize = 4 * 1024 * 1024;
const MAX_BODY_LEASES: usize = 32;
const MAX_INDEX_BYTES: u64 = 4 * 1024 * 1024;
const MAX_DIRECTORY_ENTRIES: usize = (MAX_UNSETTLED + MAX_BODY_LEASES) * 3 + 4;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct UserSendAuthority {
    pub(crate) tab_id: String,
    pub(crate) pty_id: Option<String>,
    pub(crate) binding_nonce: String,
    pub(crate) pty_session_generation: Option<u64>,
    pub(crate) provider_id: String,
    pub(crate) session_id: Option<String>,
    pub(crate) observed_session_id: Option<String>,
    #[serde(default)]
    pub(crate) prelaunch: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UserSendState {
    Queued,
    Claimed,
    Uncertain,
    BodyLease,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct UserSendRecord {
    pub(crate) send_id: String,
    pub(crate) client_token: Option<String>,
    pub(crate) acceptance_sequence: u64,
    pub(crate) ledger_sequence: u64,
    pub(crate) authority: UserSendAuthority,
    pub(crate) owner_epoch: String,
    pub(crate) reservation: u64,
    #[serde(default = "held_before_readiness_by_default")]
    pub(crate) held_before_readiness: bool,
    pub(crate) state: UserSendState,
    pub(crate) handoff_session: Option<String>,
    pub(crate) typed_follow_up: Option<String>,
    original_digest: String,
    original_bytes: usize,
    prepared_digest: String,
    prepared_bytes: usize,
    body_digest: Option<String>,
    body_path: Option<String>,
    body_bytes: usize,
}

fn held_before_readiness_by_default() -> bool {
    true
}

#[derive(Clone, Serialize, Deserialize)]
struct UserIndex {
    version: u8,
    next_sequence: u64,
    records: Vec<UserSendRecord>,
    #[serde(default)]
    receipts: Vec<continuity::UserSendReceipt>,
    #[serde(default)]
    runtime_questions: crate::service::agent_runtime::questions::QuestionBook,
}

impl Default for UserIndex {
    fn default() -> Self {
        Self {
            version: 1,
            next_sequence: 1,
            records: Vec::new(),
            receipts: Vec::new(),
            runtime_questions: Default::default(),
        }
    }
}

struct LoadedIndex {
    index: UserIndex,
    sequence_base: u64,
}

#[derive(Default)]
struct Reservations {
    next: u64,
    pending: BTreeSet<u64>,
    tokens: AdmissionTokens,
}

pub(crate) struct UserTab {
    tab_id: String,
    dir: PathBuf,
    data: Mutex<Option<LoadedIndex>>,
    admissions: Mutex<Reservations>,
    unsettled: AtomicUsize,
    pub(crate) revoked: AtomicBool,
    pub(crate) failed: AtomicBool,
    pub(crate) running: AtomicBool,
    pub(crate) waiting_for_dispatch: AtomicBool,
    pub(crate) wake_generation: AtomicU64,
    pub(crate) dispatch_generation: AtomicU64,
    pub(crate) dispatch_events: tokio::sync::watch::Sender<u64>,
    ready_launch: Mutex<ReadyLaunchLatch>,
    ended_launch_nonce: Mutex<Option<String>>,
    admission_wake: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

pub(crate) struct UserReservation {
    pub(crate) send_id: String,
    pub(crate) ledger_sequence: u64,
    pub(crate) authority: UserSendAuthority,
    pub(crate) client_token: Option<String>,
    ordinal: u64,
    tab: Arc<UserTab>,
    reused: bool,
    token_entry: Option<Arc<TokenEntry>>,
}

impl UserReservation {
    pub(crate) fn wait_existing(&self) -> Result<bool, String> {
        if self.reused {
            self.token_entry
                .as_ref()
                .ok_or("reused token is absent")?
                .wait()?;
        }
        Ok(self.reused)
    }
}

impl Drop for UserReservation {
    fn drop(&mut self) {
        if self.reused {
            return;
        }
        self.tab.admissions.lock().pending.remove(&self.ordinal);
        if let Some(entry) = &self.token_entry {
            entry.finish();
        }
        let wake = self.tab.admission_wake.lock().clone();
        if let Some(wake) = wake {
            wake();
        }
    }
}

pub(crate) struct PreparedUserBody {
    pub(crate) returned_path: PathBuf,
    pub(crate) bytes: Vec<u8>,
}

pub(crate) struct UserSendStore {
    root: PathBuf,
    root_initialized: Mutex<bool>,
    epoch: String,
    tabs: Mutex<HashMap<String, Arc<UserTab>>>,
    #[cfg(test)]
    persistence_boundary: Mutex<Option<Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>>>,
}

impl UserSendStore {
    pub(crate) fn open(data_dir: &Path) -> Result<Self, String> {
        let root = data_dir.join("user-send-payloads");
        Ok(Self {
            root,
            root_initialized: Mutex::new(false),
            epoch: uuid::Uuid::new_v4().to_string(),
            tabs: Mutex::new(HashMap::new()),
            #[cfg(test)]
            persistence_boundary: Mutex::new(None),
        })
    }

    pub(crate) fn tab(&self, tab_id: &str) -> Arc<UserTab> {
        self.tabs
            .lock()
            .entry(tab_id.to_string())
            .or_insert_with(|| {
                let (dispatch_events, _) = tokio::sync::watch::channel(0);
                Arc::new(UserTab {
                    tab_id: tab_id.to_string(),
                    dir: self.root.join(digest(tab_id.as_bytes())),
                    data: Mutex::new(None),
                    admissions: Mutex::new(Reservations::default()),
                    unsettled: AtomicUsize::new(0),
                    revoked: AtomicBool::new(false),
                    failed: AtomicBool::new(false),
                    running: AtomicBool::new(false),
                    waiting_for_dispatch: AtomicBool::new(false),
                    wake_generation: AtomicU64::new(0),
                    dispatch_generation: AtomicU64::new(0),
                    dispatch_events,
                    ready_launch: Mutex::default(),
                    ended_launch_nonce: Mutex::new(None),
                    admission_wake: Mutex::new(None),
                })
            })
            .clone()
    }

    pub(crate) fn latch_ready_launch(&self, authority: &UserSendAuthority) -> bool {
        self.tab(&authority.tab_id)
            .ready_launch
            .lock()
            .mark_ready(authority)
    }

    pub(crate) fn ready_launch_is_latched(&self, authority: &UserSendAuthority) -> bool {
        self.tab(&authority.tab_id)
            .ready_launch
            .lock()
            .is_ready(authority)
    }

    pub(crate) fn set_admission_wake(&self, tab_id: &str, wake: Arc<dyn Fn() + Send + Sync>) {
        *self.tab(tab_id).admission_wake.lock() = Some(wake);
    }

    pub(crate) fn admit(
        &self,
        reservation: &UserReservation,
        original: &str,
        prepared: &str,
        body: Option<PreparedUserBody>,
    ) -> Result<UserSendRecord, String> {
        self.admit_with_follow_up(reservation, original, prepared, body, None, true)
    }

    pub(crate) fn final_transport(
        &self,
        reservation: &UserReservation,
        prepared: &str,
        body: Option<&PreparedUserBody>,
    ) -> Result<String, String> {
        if prepared.len() > MAX_PREPARED_BYTES {
            return Err("prepared transport exceeds storage bound".into());
        }
        let prepared = match body {
            Some(body) => rebase_prepared_pointer(
                prepared,
                &body.returned_path,
                &reservation.tab.payload_path(&reservation.send_id, "body")?,
            )?,
            None => prepared.to_string(),
        };
        Ok(super::super::pty_io::sanitize_agent_prompt_input(&prepared))
    }

    pub(crate) fn admit_with_follow_up(
        &self,
        reservation: &UserReservation,
        original: &str,
        prepared: &str,
        body: Option<PreparedUserBody>,
        typed_follow_up: Option<String>,
        held_before_readiness: bool,
    ) -> Result<UserSendRecord, String> {
        self.admit_with_question(
            reservation,
            original,
            prepared,
            body,
            typed_follow_up,
            held_before_readiness,
            None,
        )
    }

    fn admit_with_question(
        &self,
        reservation: &UserReservation,
        original: &str,
        prepared: &str,
        body: Option<PreparedUserBody>,
        typed_follow_up: Option<String>,
        held_before_readiness: bool,
        question: Option<runtime_questions::QuestionResolution>,
    ) -> Result<UserSendRecord, String> {
        if reservation.reused {
            return Err("duplicate reservation must resolve the existing ledger status".into());
        }
        if original.len() > MAX_PREPARED_BYTES
            || prepared.len() > MAX_PREPARED_BYTES
            || body
                .as_ref()
                .is_some_and(|body| body.bytes.len() > MAX_PREPARED_BYTES)
            || typed_follow_up
                .as_ref()
                .is_some_and(|text| text.len() > MAX_PREPARED_BYTES)
        {
            return Err("user payload exceeds bounded storage work".into());
        }
        let tab = &reservation.tab;
        let mut data = tab.data.lock();
        let loaded = self.load(tab, &mut data)?;
        if tab.revoked.load(Ordering::Acquire) || tab.failed.load(Ordering::Acquire) {
            return Err("user send owner is held or revoked".into());
        }
        if tab.ended_launch_nonce.lock().as_deref()
            == Some(reservation.authority.binding_nonce.as_str())
        {
            return Err("launch_ended_before_write".into());
        }
        let body_path = tab.payload_path(&reservation.send_id, "body")?;
        let prepared = self.final_transport(reservation, prepared, body.as_ref())?;
        let acceptance_sequence = loaded
            .sequence_base
            .checked_add(reservation.ledger_sequence)
            .ok_or("user acceptance sequence exhausted")?;
        let record = UserSendRecord {
            send_id: reservation.send_id.clone(),
            client_token: reservation.client_token.clone(),
            acceptance_sequence,
            ledger_sequence: reservation.ledger_sequence,
            authority: reservation.authority.clone(),
            owner_epoch: self.epoch.clone(),
            reservation: reservation.ordinal,
            held_before_readiness,
            state: UserSendState::Queued,
            handoff_session: None,
            typed_follow_up,
            original_digest: digest(original.as_bytes()),
            original_bytes: original.len(),
            prepared_digest: digest(prepared.as_bytes()),
            prepared_bytes: prepared.len(),
            body_digest: body.as_ref().map(|body| digest(&body.bytes)),
            body_path: body
                .as_ref()
                .map(|_| {
                    body_path
                        .to_str()
                        .map(|path| codeterm_paths::strip_verbatim_prefix(path).to_string())
                        .ok_or("durable prepared path is not Unicode")
                })
                .transpose()?,
            body_bytes: body.as_ref().map_or(0, |body| body.bytes.len()),
        };
        let mut next = loaded.index.clone();
        if let Some(question) = question {
            if next
                .runtime_questions
                .get(&question.thread_id, &question.item_id)
                .and_then(|record| record.resolved.as_ref())
                .is_some_and(|receipt| receipt.send_id != reservation.send_id)
            {
                return Err("Runtime question already has a durable answer receipt".into());
            }
            next.runtime_questions
                .resolve(&question.thread_id, &question.item_id, question.receipt)
                .map_err(|reason| reason.to_string())?;
        }
        next.records.push(record.clone());
        next.records
            .sort_by_key(|record| record.acceptance_sequence);
        next.next_sequence = next.next_sequence.max(
            acceptance_sequence
                .checked_add(1)
                .ok_or("user acceptance sequence exhausted")?,
        );
        validate_index(&next, &record.authority.tab_id)?;
        let persisted = (|| {
            self.boundary("payload")?;
            write_new(
                &tab.payload_path(&record.send_id, "original")?,
                original.as_bytes(),
            )?;
            write_new(
                &tab.payload_path(&record.send_id, "prepared")?,
                prepared.as_bytes(),
            )?;
            if let Some(body) = body {
                write_new(&body_path, &body.bytes)?;
            }
            sync_directory(&tab.dir)?;
            self.persist(tab, &next).map_err(|error| {
                format!("admission_unknown: durability could not be qualified; owner held: {error}")
            })
        })();
        if let Err(error) = persisted {
            tab.failed.store(true, Ordering::Release);
            return Err(error);
        }
        loaded.index = next;
        tab.unsettled
            .store(unsettled_count(&loaded.index), Ordering::Release);
        if tab.revoked.load(Ordering::Acquire) {
            return Err("admission_unknown: user admission persisted across owner revocation; held uncertainty".into());
        }
        Ok(record)
    }

    pub(crate) fn records(&self, tab_id: &str) -> Result<Vec<UserSendRecord>, String> {
        let tab = self.tab(tab_id);
        let mut data = tab.data.lock();
        Ok(self.load(&tab, &mut data)?.index.records.clone())
    }

    pub(crate) fn head(&self, tab_id: &str) -> Result<Option<UserSendRecord>, String> {
        Ok(self
            .records(tab_id)?
            .into_iter()
            .find(|record| record.state != UserSendState::BodyLease))
    }

    pub(crate) fn queued(&self, tab_id: &str) -> Result<Vec<UserSendRecord>, String> {
        Ok(self
            .records(tab_id)?
            .into_iter()
            .filter(|record| record.state == UserSendState::Queued)
            .collect())
    }

    pub(crate) fn has_unsettled(&self, tab_id: &str) -> bool {
        self.tabs
            .lock()
            .get(tab_id)
            .is_some_and(|tab| tab.unsettled.load(Ordering::Acquire) > 0)
    }

    pub(crate) fn bind_pty_session_generation(
        &self,
        record: &UserSendRecord,
        generation: u64,
    ) -> Result<Option<UserSendRecord>, String> {
        let tab = self.tab(&record.authority.tab_id);
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        let Some(current) = loaded
            .index
            .records
            .iter()
            .find(|item| item.send_id == record.send_id)
        else {
            return Ok(None);
        };
        if current.authority != record.authority || current.state != UserSendState::Queued {
            return Ok(None);
        }
        if let Some(current_generation) = current.authority.pty_session_generation {
            return Ok((current_generation == generation).then(|| current.clone()));
        }
        let mut next = loaded.index.clone();
        let current = next
            .records
            .iter_mut()
            .find(|item| item.send_id == record.send_id)
            .ok_or("user head disappeared")?;
        current.authority.pty_session_generation = Some(generation);
        let bound = current.clone();
        self.persist(&tab, &next)?;
        loaded.index = next;
        Ok(Some(bound))
    }

    pub(crate) fn rebind_queued_authority(
        &self,
        record: &UserSendRecord,
        authority: UserSendAuthority,
    ) -> Result<Option<UserSendRecord>, String> {
        let tab = self.tab(&record.authority.tab_id);
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        let Some(current) = loaded
            .index
            .records
            .iter()
            .find(|item| item.send_id == record.send_id)
        else {
            return Ok(None);
        };
        if current.authority != record.authority || current.state != UserSendState::Queued {
            return Ok(None);
        }
        let mut next = loaded.index.clone();
        let current = next
            .records
            .iter_mut()
            .find(|item| item.send_id == record.send_id)
            .ok_or("user head disappeared")?;
        current.authority = authority;
        let rebound = current.clone();
        self.persist(&tab, &next)?;
        loaded.index = next;
        Ok(Some(rebound))
    }

    pub(crate) fn eligible(&self, record: &UserSendRecord) -> bool {
        let tab = self.tab(&record.authority.tab_id);
        record.owner_epoch == self.epoch
            && record.state == UserSendState::Queued
            && !tab.revoked.load(Ordering::Acquire)
            && !tab.failed.load(Ordering::Acquire)
            && !tab
                .admissions
                .lock()
                .pending
                .iter()
                .any(|ordinal| *ordinal <= record.reservation)
    }

    pub(crate) fn claim<'a>(
        &self,
        record: &UserSendRecord,
        session: impl Into<Option<&'a str>>,
    ) -> Result<bool, String> {
        let session = session.into();
        let tab = self.tab(&record.authority.tab_id);
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        if !self.eligible(record) {
            return Ok(false);
        }
        let Some(current) = loaded
            .index
            .records
            .iter()
            .find(|item| item.send_id == record.send_id)
        else {
            return Ok(false);
        };
        if current.state != UserSendState::Queued
            || current.authority != record.authority
            || current.owner_epoch != record.owner_epoch
            || session.is_some_and(|session| {
                current
                    .authority
                    .session_id
                    .as_deref()
                    .is_some_and(|expected| expected != session)
            })
        {
            return Ok(false);
        }
        let mut next = loaded.index.clone();
        let current = next
            .records
            .iter_mut()
            .find(|item| item.send_id == record.send_id)
            .ok_or("user send disappeared")?;
        current.state = UserSendState::Claimed;
        current.handoff_session = session.map(str::to_string);
        self.persist(&tab, &next)?;
        loaded.index = next;
        Ok(true)
    }

    pub(crate) fn finish_handoff(
        &self,
        record: &UserSendRecord,
        proven_zero_write: bool,
    ) -> Result<(), String> {
        let tab = self.tab(&record.authority.tab_id);
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        let mut next = loaded.index.clone();
        let current = next
            .records
            .iter_mut()
            .find(|item| item.send_id == record.send_id)
            .ok_or("user claim disappeared")?;
        if current.state != UserSendState::Claimed
            || current.authority != record.authority
            || current.owner_epoch != self.epoch
            || record.owner_epoch != self.epoch
        {
            return Err("user claim is not active in this owner epoch".into());
        }
        current.state = if proven_zero_write {
            UserSendState::Queued
        } else {
            UserSendState::Uncertain
        };
        if proven_zero_write {
            current.handoff_session = None;
        }
        self.persist(&tab, &next)?;
        loaded.index = next;
        Ok(())
    }

    pub(crate) fn discard_queued_from_ended_launch(
        &self,
        record: &UserSendRecord,
    ) -> Result<bool, String> {
        let tab = self.tab(&record.authority.tab_id);
        if tab.revoked.load(Ordering::Acquire) || tab.failed.load(Ordering::Acquire) {
            return Ok(false);
        }
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        let Some(current) = loaded
            .index
            .records
            .iter()
            .find(|item| item.send_id == record.send_id)
        else {
            return Ok(false);
        };
        if current.authority != record.authority
            || current.state != UserSendState::Queued
            || current.owner_epoch != record.owner_epoch
        {
            return Ok(false);
        }
        let mut next = loaded.index.clone();
        next.records.retain(|item| item.send_id != record.send_id);
        continuity::retain_receipt(&mut next, record, continuity::ReceiptState::Failed);
        self.persist(&tab, &next)?;
        loaded.index = next;
        tab.unsettled
            .store(unsettled_count(&loaded.index), Ordering::Release);
        let cleanup = (|| {
            for suffix in ["original", "prepared", "body"] {
                let path = tab.payload_path(&record.send_id, suffix)?;
                match fs::remove_file(path) {
                    Ok(()) => (),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                    Err(error) => return Err(io_error(error)),
                }
            }
            sync_directory(&tab.dir)
        })();
        if let Err(error) = cleanup {
            log::warn!(
                "[chat-send] expired user send {} payload cleanup failed: {error}",
                record.send_id
            );
        }
        Ok(true)
    }

    #[cfg(test)]
    pub(crate) fn settle_cycle(
        &self,
        record: &UserSendRecord,
        proven_zero_write: bool,
    ) -> Result<(), String> {
        self.finish_handoff(record, proven_zero_write)
    }

    pub(crate) fn complete(&self, record: &UserSendRecord) -> Result<bool, String> {
        let tab = self.tab(&record.authority.tab_id);
        if tab.revoked.load(Ordering::Acquire)
            || tab.failed.load(Ordering::Acquire)
            || record.owner_epoch != self.epoch
        {
            return Ok(false);
        }
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        let Some(current) = loaded
            .index
            .records
            .iter()
            .find(|item| item.send_id == record.send_id)
        else {
            return Ok(false);
        };
        if current.authority != record.authority || current.state != UserSendState::Uncertain {
            return Ok(false);
        }
        let mut next = loaded.index.clone();
        continuity::retain_receipt(&mut next, record, continuity::ReceiptState::Uncertain);
        if record.body_digest.is_some() {
            let leases = next
                .records
                .iter()
                .filter(|item| item.state == UserSendState::BodyLease)
                .count();
            if leases >= MAX_BODY_LEASES {
                return Err("prepared body lease capacity reached".into());
            }
            next.records
                .iter_mut()
                .find(|item| item.send_id == record.send_id)
                .ok_or("user send disappeared")?
                .state = UserSendState::BodyLease;
        } else {
            next.records.retain(|head| head.send_id != record.send_id);
        }
        self.persist(&tab, &next)?;
        loaded.index = next;
        tab.unsettled
            .store(unsettled_count(&loaded.index), Ordering::Release);
        if record.body_digest.is_none() {
            let cleanup = (|| {
                for suffix in ["original", "prepared"] {
                    fs::remove_file(tab.payload_path(&record.send_id, suffix)?)
                        .map_err(io_error)?;
                }
                sync_directory(&tab.dir)
            })();
            if let Err(error) = cleanup {
                tab.failed.store(true, Ordering::Release);
                return Err(error);
            }
        }
        Ok(true)
    }

    #[cfg(test)]
    pub(crate) fn original(&self, tab_id: &str, send_id: &str) -> Result<String, String> {
        let record = self
            .records(tab_id)?
            .into_iter()
            .find(|record| record.send_id == send_id)
            .ok_or("user send is not indexed")?;
        self.read_payload(
            &record,
            "original",
            &record.original_digest,
            record.original_bytes,
        )
    }

    pub(crate) fn prepared(&self, record: &UserSendRecord) -> Result<String, String> {
        if let Some(expected) = &record.body_digest {
            let bytes = read_bounded(
                &self
                    .tab(&record.authority.tab_id)
                    .payload_path(&record.send_id, "body")?,
                MAX_PREPARED_BYTES as u64,
            )?;
            if bytes.len() != record.body_bytes || digest(&bytes) != *expected {
                return Err("prepared body integrity failed".into());
            }
        }
        self.read_payload(
            record,
            "prepared",
            &record.prepared_digest,
            record.prepared_bytes,
        )
    }

    pub(crate) fn body_path(&self, record: &UserSendRecord) -> Result<Option<PathBuf>, String> {
        Ok(record.body_path.as_ref().map(PathBuf::from))
    }

    fn read_payload(
        &self,
        record: &UserSendRecord,
        suffix: &str,
        expected: &str,
        length: usize,
    ) -> Result<String, String> {
        let bytes = read_bounded(
            &self
                .tab(&record.authority.tab_id)
                .payload_path(&record.send_id, suffix)?,
            MAX_PREPARED_BYTES as u64,
        )?;
        if bytes.len() != length || digest(&bytes) != expected {
            return Err("user payload integrity failed".into());
        }
        String::from_utf8(bytes).map_err(|_| "user payload is not UTF-8".into())
    }

    fn load<'a>(
        &self,
        tab: &UserTab,
        data: &'a mut Option<LoadedIndex>,
    ) -> Result<&'a mut LoadedIndex, String> {
        if data.is_none() {
            {
                let mut initialized = self.root_initialized.lock();
                if !*initialized {
                    owner_directory(&self.root)?;
                    *initialized = true;
                }
            }
            owner_directory(&tab.dir)?;
            let path = tab.dir.join("index.json");
            let index: UserIndex = match fs::symlink_metadata(&path) {
                Ok(_) => serde_json::from_slice(&read_bounded(&path, MAX_INDEX_BYTES)?)
                    .map_err(|_| "user index is invalid")?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    let mut entries = fs::read_dir(&tab.dir).map_err(io_error)?;
                    if entries.next().transpose().map_err(io_error)?.is_some() {
                        return Err("user index absent with retained files; owner held".into());
                    }
                    UserIndex::default()
                }
                Err(error) => return Err(io_error(error)),
            };
            if index.version != 1 {
                return Err("user index version is unknown".into());
            }
            validate_index(&index, &tab.tab_id)?;
            let sequence_base = index.next_sequence;
            tab.unsettled
                .store(unsettled_count(&index), Ordering::Release);
            *data = Some(LoadedIndex {
                index,
                sequence_base,
            });
            if let Err(error) =
                self.collect_orphans(tab, &data.as_ref().ok_or("user index not loaded")?.index)
            {
                tab.failed.store(true, Ordering::Release);
                return Err(error);
            }
        }
        data.as_mut().ok_or_else(|| "user index not loaded".into())
    }

    fn collect_orphans(&self, tab: &UserTab, index: &UserIndex) -> Result<(), String> {
        let entries = fs::read_dir(&tab.dir)
            .map_err(io_error)?
            .take(MAX_DIRECTORY_ENTRIES + 1)
            .collect::<Result<Vec<_>, _>>()
            .map_err(io_error)?;
        if entries.len() > MAX_DIRECTORY_ENTRIES {
            return Err("user directory work bound exceeded".into());
        }
        let live: HashSet<String> = index
            .records
            .iter()
            .flat_map(|record| {
                ["original", "prepared", "body"]
                    .map(|suffix| format!("{}.{suffix}", record.send_id))
            })
            .collect();
        for entry in entries {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name == "index.json" || live.contains(&name) {
                continue;
            }
            let owned = name == "index.next.json"
                || name.rsplit_once('.').is_some_and(|(id, suffix)| {
                    uuid::Uuid::parse_str(id).is_ok()
                        && matches!(suffix, "original" | "prepared" | "body")
                });
            if owned && entry.file_type().map_err(io_error)?.is_file() {
                fs::remove_file(entry.path()).map_err(io_error)?;
            }
        }
        Ok(())
    }

    fn persist(&self, tab: &UserTab, index: &UserIndex) -> Result<(), String> {
        let result = (|| {
            self.boundary("index")?;
            let bytes = serde_json::to_vec(index).map_err(|_| "user index serialization failed")?;
            if bytes.len() as u64 > MAX_INDEX_BYTES {
                return Err("user index size bound exceeded".into());
            }
            let temporary = tab.dir.join("index.next.json");
            write_replace(&temporary, &bytes)?;
            atomic_replace(&temporary, &tab.dir.join("index.json"))?;
            sync_directory(&tab.dir)
        })();
        if result.is_err() {
            tab.failed.store(true, Ordering::Release);
        }
        result
    }

    fn boundary(&self, _stage: &str) -> Result<(), String> {
        #[cfg(test)]
        {
            let boundary = self.persistence_boundary.lock().clone();
            if let Some(boundary) = boundary {
                return boundary(_stage);
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_persistence_boundary(
        &self,
        boundary: Option<Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>>,
    ) {
        *self.persistence_boundary.lock() = boundary;
    }
}

impl UserTab {
    fn payload_path(&self, send_id: &str, suffix: &str) -> Result<PathBuf, String> {
        if uuid::Uuid::parse_str(send_id).is_err() {
            return Err("invalid user send identity".into());
        }
        Ok(self.dir.join(format!("{send_id}.{suffix}")))
    }
}

fn validate_index(index: &UserIndex, tab_id: &str) -> Result<(), String> {
    continuity::validate_receipts(index, tab_id)?;
    if index.next_sequence == 0 {
        return Err("user index sequence is invalid".into());
    }
    if serde_json::to_vec(index)
        .map_err(|_| "user index serialization failed")?
        .len() as u64
        > MAX_INDEX_BYTES
    {
        return Err("user index metadata capacity exceeded".into());
    }
    if unsettled_count(index) > MAX_UNSETTLED
        || index.records.len() > MAX_UNSETTLED + MAX_BODY_LEASES
        || index
            .records
            .iter()
            .filter(|record| record.body_digest.is_some())
            .count()
            > MAX_BODY_LEASES
    {
        return Err("user send capacity: 32 unsettled records".into());
    }
    let mut ids = HashSet::new();
    let mut sequences = HashSet::new();
    let mut prepared = 0usize;
    let mut original = 0usize;
    for record in &index.records {
        if record.authority.tab_id != tab_id
            || uuid::Uuid::parse_str(&record.send_id).is_err()
            || !ids.insert(&record.send_id)
            || !sequences.insert(record.acceptance_sequence)
            || record.acceptance_sequence >= index.next_sequence
            || (record.state == UserSendState::BodyLease && record.body_digest.is_none())
            || record.body_digest.is_some() != record.body_path.is_some()
            || record.original_digest.len() != 64
            || record.prepared_digest.len() != 64
            || record
                .body_digest
                .as_ref()
                .is_some_and(|digest| digest.len() != 64)
        {
            return Err("user index identity is invalid".into());
        }
        prepared = prepared
            .checked_add(record.prepared_bytes)
            .and_then(|value| {
                value.checked_add(record.typed_follow_up.as_ref().map_or(0, String::len))
            })
            .and_then(|value| value.checked_add(record.body_bytes))
            .ok_or("prepared byte accounting overflow")?;
        original = original
            .checked_add(record.original_bytes)
            .ok_or("original byte accounting overflow")?;
    }
    if index
        .records
        .windows(2)
        .any(|pair| pair[0].acceptance_sequence >= pair[1].acceptance_sequence)
    {
        return Err("user index acceptance order is invalid".into());
    }
    if prepared > MAX_PREPARED_BYTES || original > MAX_PREPARED_BYTES {
        return Err("user send capacity: 4 MiB aggregate prepared/body and original bytes".into());
    }
    Ok(())
}

fn unsettled_count(index: &UserIndex) -> usize {
    index
        .records
        .iter()
        .filter(|record| record.state != UserSendState::BodyLease)
        .count()
}

fn rebase_prepared_pointer(
    prepared: &str,
    returned: &Path,
    durable: &Path,
) -> Result<String, String> {
    let returned = returned.to_str().ok_or("prepared path is not Unicode")?;
    let durable = durable
        .to_str()
        .ok_or("durable prepared path is not Unicode")?;
    let expected = format!("Read and follow the message in {returned}");
    if prepared != expected {
        return Err("prepared pointer shape is not owned".into());
    }
    Ok(format!(
        "Read and follow the message in {}",
        codeterm_paths::strip_verbatim_prefix(durable)
    ))
}

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn io_error(error: std::io::Error) -> String {
    format!("user send persistence: {error}")
}

pub(crate) fn read_bounded(path: &Path, maximum: u64) -> Result<Vec<u8>, String> {
    if !fs::symlink_metadata(path)
        .map_err(io_error)?
        .file_type()
        .is_file()
    {
        return Err("user payload/index is not a regular file".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path).map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.file_type().is_file() {
        return Err("opened user payload/index is not a regular file".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
        {
            return Err("opened user payload/index is a reparse point".into());
        }
    }
    let mut bytes = Vec::new();
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > maximum {
        return Err("user payload/index size bound exceeded".into());
    }
    Ok(bytes)
}

fn validate_owner_directory(metadata: &fs::Metadata) -> Result<(), String> {
    // Rust types a junction as a symlink, so the reparse attribute must be read before is_dir.
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
        {
            return Err("user owner directory is a reparse point".into());
        }
    }
    if !metadata.file_type().is_dir() {
        return Err("user owner directory is not an owned regular directory".into());
    }
    Ok(())
}

pub(super) fn owner_directory(path: &Path) -> Result<(), String> {
    let existed = match fs::symlink_metadata(path) {
        Ok(metadata) => {
            validate_owner_directory(&metadata)?;
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(io_error(error)),
    };
    fs::create_dir_all(path).map_err(io_error)?;
    validate_owner_directory(&fs::symlink_metadata(path).map_err(io_error)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(io_error)?;
    }
    #[cfg(windows)]
    {
        super::restrict_windows_acl(path).map_err(io_error)?;
    }
    if !existed {
        if let Some(parent) = path.parent() {
            sync_directory(parent)?;
        }
    }
    Ok(())
}

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(io_error)?;
    file.write_all(bytes).map_err(io_error)?;
    file.sync_all().map_err(io_error)
}

pub(super) fn write_replace(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if path.exists() {
        fs::remove_file(path).map_err(io_error)?;
    }
    write_new(path, bytes)
}

#[cfg(not(windows))]
pub(super) fn atomic_replace(from: &Path, to: &Path) -> Result<(), String> {
    fs::rename(from, to).map_err(io_error)
}

#[cfg(windows)]
pub(super) fn atomic_replace(from: &Path, to: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    let result = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(unix)]
pub(super) fn sync_directory(path: &Path) -> Result<(), String> {
    std::fs::File::open(path)
        .map_err(io_error)?
        .sync_all()
        .map_err(io_error)
}

#[cfg(not(unix))]
pub(super) fn sync_directory(_: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests;
