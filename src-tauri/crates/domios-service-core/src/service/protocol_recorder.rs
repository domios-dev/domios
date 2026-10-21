use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CHANNEL_CAPACITY: usize = 16;
const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_FILES_PER_TAB: usize = 10;
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
const RETENTION: Duration = Duration::from_secs(14 * 24 * 60 * 60);

type Clock = Arc<dyn Fn() -> SystemTime + Send + Sync>;

struct Limits {
    file_bytes: u64,
    files_per_tab: usize,
    total_bytes: u64,
    retention: Duration,
    queue_capacity: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            file_bytes: MAX_FILE_BYTES,
            files_per_tab: MAX_FILES_PER_TAB,
            total_bytes: MAX_TOTAL_BYTES,
            retention: RETENTION,
            queue_capacity: CHANNEL_CAPACITY,
        }
    }
}

struct HookPost {
    tab_id: String,
    event: String,
    body: serde_json::Value,
}

enum WriterMessage {
    HookPost(HookPost),
    #[cfg(test)]
    Barrier(SyncSender<()>),
}

struct FileEntry {
    path: PathBuf,
    tab: String,
    bytes: u64,
    modified: SystemTime,
    order: u64,
}

struct WriterState {
    root: PathBuf,
    limits: Limits,
    clock: Clock,
    active: HashMap<String, PathBuf>,
    files: Vec<FileEntry>,
    next_order: u64,
    initialized: bool,
}

pub struct ProtocolRecorder {
    sender: Option<SyncSender<WriterMessage>>,
    dropped: Arc<AtomicU64>,
    drop_warning_pending: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl ProtocolRecorder {
    pub(crate) fn new(root: impl Into<PathBuf>, dev_instance: bool) -> Self {
        if !dev_instance {
            return Self::disabled();
        }
        #[cfg(test)]
        {
            Self::start(
                root.into(),
                Limits::default(),
                Arc::new(SystemTime::now),
                None,
            )
        }
        #[cfg(not(test))]
        {
            Self::start(root.into(), Limits::default(), Arc::new(SystemTime::now))
        }
    }

    fn disabled() -> Self {
        Self {
            sender: None,
            dropped: Arc::new(AtomicU64::new(0)),
            drop_warning_pending: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
        }
    }

    fn start(
        root: PathBuf,
        limits: Limits,
        clock: Clock,
        #[cfg(test)] gate: Option<(SyncSender<()>, mpsc::Receiver<()>)>,
    ) -> Self {
        let (sender, receiver) = mpsc::sync_channel(limits.queue_capacity);
        let dropped = Arc::new(AtomicU64::new(0));
        let drop_warning_pending = Arc::new(AtomicBool::new(false));
        let drop_warning_logged = Arc::new(AtomicBool::new(false));
        let write_warning_logged = Arc::new(AtomicBool::new(false));
        let thread_dropped = dropped.clone();
        let thread_pending = drop_warning_pending.clone();
        let thread_drop_logged = drop_warning_logged.clone();
        let thread_write_logged = write_warning_logged.clone();
        let worker = std::thread::Builder::new()
            .name("protocol-recorder".to_string())
            .spawn(move || {
                let mut state = WriterState::load(root, limits, clock);
                #[cfg(test)]
                let mut gate = gate;
                while let Ok(message) = receiver.recv() {
                    if thread_pending.swap(false, Ordering::AcqRel)
                        && !thread_drop_logged.swap(true, Ordering::AcqRel)
                    {
                        log::warn!(
                            "[protocol-recorder] bounded queue dropped hook posts; dropped={}",
                            thread_dropped.load(Ordering::Relaxed)
                        );
                    }
                    match message {
                        WriterMessage::HookPost(post) => {
                            #[cfg(test)]
                            if let Some((entered, release)) = gate.take() {
                                let _ = entered.send(());
                                let _ = release.recv();
                            }
                            if let Err(error) = state.write(post) {
                                thread_dropped.fetch_add(1, Ordering::Relaxed);
                                if !thread_write_logged.swap(true, Ordering::AcqRel) {
                                    log::warn!("[protocol-recorder] write failed: {error}");
                                }
                            }
                        }
                        #[cfg(test)]
                        WriterMessage::Barrier(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            });
        let worker = match worker {
            Ok(worker) => Some(worker),
            Err(error) => {
                log::warn!("[protocol-recorder] writer thread could not start: {error}");
                return Self::disabled();
            }
        };
        Self {
            sender: Some(sender),
            dropped,
            drop_warning_pending,
            worker: Mutex::new(worker),
        }
    }

    pub fn try_record_hook_post(
        &self,
        enabled: bool,
        tab_id: &str,
        event: &str,
        body: &serde_json::Value,
    ) -> bool {
        if !enabled {
            return false;
        }
        let Some(sender) = &self.sender else {
            return false;
        };
        let post = HookPost {
            tab_id: tab_id.to_string(),
            event: event.to_string(),
            body: body.clone(),
        };
        match sender.try_send(WriterMessage::HookPost(post)) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                self.drop_warning_pending.store(true, Ordering::Release);
                false
            }
        }
    }

    #[cfg(test)]
    fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    fn barrier(&self) {
        let (done, received) = mpsc::sync_channel(1);
        self.sender
            .as_ref()
            .unwrap()
            .send(WriterMessage::Barrier(done))
            .unwrap();
        received.recv_timeout(Duration::from_secs(2)).unwrap();
    }
}

impl Drop for ProtocolRecorder {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.lock().unwrap().take() {
            let _ = worker.join();
        }
    }
}

impl WriterState {
    fn load(root: PathBuf, limits: Limits, clock: Clock) -> Self {
        Self {
            root,
            limits,
            clock,
            active: HashMap::new(),
            files: Vec::new(),
            next_order: 0,
            initialized: false,
        }
    }

    fn scan_existing(&mut self) {
        let Ok(tabs) = fs::read_dir(&self.root) else {
            return;
        };
        for tab_entry in tabs.flatten() {
            let Ok(kind) = tab_entry.file_type() else {
                continue;
            };
            if !kind.is_dir() {
                continue;
            }
            let tab = tab_entry.file_name().to_string_lossy().into_owned();
            let Ok(files) = fs::read_dir(tab_entry.path()) else {
                continue;
            };
            for file in files.flatten() {
                let name = file.file_name();
                if !name.to_string_lossy().starts_with("protocol-")
                    || !name.to_string_lossy().ends_with(".ndjson")
                {
                    continue;
                }
                let Ok(kind) = file.file_type() else {
                    continue;
                };
                if !kind.is_file() {
                    continue;
                }
                let Ok(metadata) = file.metadata() else {
                    continue;
                };
                self.next_order += 1;
                let entry = FileEntry {
                    path: file.path(),
                    tab: tab.clone(),
                    bytes: metadata.len(),
                    modified: metadata.modified().unwrap_or(UNIX_EPOCH),
                    order: self.next_order,
                };
                self.files.push(entry);
            }
        }
        self.files.sort_by(|left, right| {
            left.modified
                .cmp(&right.modified)
                .then_with(|| left.path.cmp(&right.path))
        });
        for entry in &mut self.files {
            self.next_order += 1;
            entry.order = self.next_order;
        }
        let mut newest = HashMap::<String, (u64, PathBuf)>::new();
        for entry in &self.files {
            let candidate = (entry.order, entry.path.clone());
            if newest
                .get(&entry.tab)
                .is_none_or(|current| current.0 < candidate.0)
            {
                newest.insert(entry.tab.clone(), candidate);
            }
        }
        for (tab, (order, path)) in newest {
            if self
                .files
                .iter()
                .find(|entry| entry.order == order)
                .is_some_and(|entry| entry.bytes < self.limits.file_bytes)
            {
                self.active.insert(tab, path);
            }
        }
    }

    fn now(&self) -> SystemTime {
        (self.clock)()
    }

    fn write(&mut self, post: HookPost) -> io::Result<()> {
        let now = self.now();
        if !self.initialized {
            self.scan_existing();
            self.prune(now);
            self.initialized = true;
        }
        let tab = tab_directory(&post.tab_id);
        let body = redact_json(&post.body);
        let timestamp = now
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let mut line = serde_json::to_vec(&serde_json::json!({
            "timestamp_ms": timestamp,
            "tab_id": post.tab_id,
            "event": post.event,
            "body": body,
        }))?;
        line.push(b'\n');
        let record_bytes = line.len() as u64;
        if record_bytes > self.limits.file_bytes || record_bytes > self.limits.total_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "hook post exceeds recorder bounds",
            ));
        }
        self.prune(now);
        let current = self.active.get(&tab).cloned();
        let current_bytes = current
            .as_ref()
            .and_then(|path| self.files.iter().find(|entry| &entry.path == path))
            .map(|entry| entry.bytes);
        let rotate =
            current_bytes.is_some_and(|bytes| bytes + record_bytes > self.limits.file_bytes);
        if rotate || self.total_bytes() + record_bytes > self.limits.total_bytes {
            self.active.remove(&tab);
        }
        while self.total_bytes() + record_bytes > self.limits.total_bytes {
            if !self.remove_oldest(None) {
                return Err(io::Error::other("protocol recorder total bound reached"));
            }
        }
        let path = match self.active.get(&tab) {
            Some(path) => path.clone(),
            None => self.new_file(&tab, timestamp)?,
        };
        let is_new = !self.files.iter().any(|entry| entry.path == path);
        let mut file = if is_new {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?
        } else {
            OpenOptions::new().append(true).open(&path)?
        };
        if let Err(error) = file.write_all(&line) {
            drop(file);
            if is_new {
                let _ = fs::remove_file(&path);
            } else if let Ok(metadata) = fs::metadata(&path) {
                if let Some(entry) = self.files.iter_mut().find(|entry| entry.path == path) {
                    entry.bytes = metadata.len();
                    entry.modified = now;
                }
            }
            self.prune(now);
            return Err(error);
        }
        drop(file);
        if is_new {
            self.active.insert(tab.clone(), path.clone());
            self.next_order += 1;
            self.files.push(FileEntry {
                path: path.clone(),
                tab: tab.clone(),
                bytes: 0,
                modified: now,
                order: self.next_order,
            });
        }
        if let Some(entry) = self.files.iter_mut().find(|entry| entry.path == path) {
            entry.bytes += record_bytes;
            entry.modified = now;
            self.next_order += 1;
            entry.order = self.next_order;
        }
        self.prune(now);
        Ok(())
    }

    fn new_file(&mut self, tab: &str, timestamp: u128) -> io::Result<PathBuf> {
        let directory = self.root.join(tab);
        fs::create_dir_all(&directory)?;
        let name = format!("protocol-{timestamp}-{}.ndjson", uuid::Uuid::new_v4());
        Ok(directory.join(name))
    }

    fn prune(&mut self, now: SystemTime) {
        let expiry = now.checked_sub(self.limits.retention).unwrap_or(UNIX_EPOCH);
        let expired = self
            .files
            .iter()
            .filter(|entry| entry.modified < expiry || entry.bytes > self.limits.file_bytes)
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        for path in expired {
            self.delete(&path);
        }
        let mut tabs = HashMap::<String, Vec<(u64, PathBuf)>>::new();
        for entry in &self.files {
            tabs.entry(entry.tab.clone())
                .or_default()
                .push((entry.order, entry.path.clone()));
        }
        for entries in tabs.values_mut() {
            entries.sort_by_key(|entry| entry.0);
            let excess = entries.len().saturating_sub(self.limits.files_per_tab);
            for (_, path) in entries.drain(..excess) {
                self.delete(&path);
            }
        }
        while self.total_bytes() > self.limits.total_bytes {
            if !self.remove_oldest(None) {
                break;
            }
        }
    }

    fn remove_oldest(&mut self, except: Option<&Path>) -> bool {
        let oldest = self
            .files
            .iter()
            .filter(|entry| except != Some(entry.path.as_path()))
            .min_by_key(|entry| (entry.modified, entry.order))
            .map(|entry| entry.path.clone());
        oldest.is_some_and(|path| self.delete(&path))
    }

    fn delete(&mut self, path: &Path) -> bool {
        let deleted = match fs::remove_file(path) {
            Ok(()) => true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => true,
            Err(_) => false,
        };
        if deleted {
            self.files.retain(|entry| entry.path != path);
            self.active.retain(|_, active| active != path);
        }
        deleted
    }

    fn total_bytes(&self) -> u64 {
        self.files.iter().map(|entry| entry.bytes).sum()
    }
}

fn redact_json(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => serde_json::Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let value = if secret_key(key) {
                        serde_json::Value::String("<redacted>".to_string())
                    } else {
                        redact_json(value)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(redact_json).collect())
        }
        serde_json::Value::String(value) => {
            serde_json::Value::String(crate::service::launch_diagnostics::redact_secrets(value))
        }
        _ => value.clone(),
    }
}

fn secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("token")
        || key.contains("secret")
        || key.contains("password")
        || key.contains("authorization")
        || key.contains("nonce")
        || key.contains("api_key")
        || key.contains("apikey")
        || key.contains("credential")
}

fn tab_directory(tab_id: &str) -> String {
    let hash = tab_id.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    let prefix = tab_id
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        .take(48)
        .collect::<String>();
    format!(
        "{}-{:016x}",
        if prefix.is_empty() { "tab" } else { &prefix },
        hash
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::AtomicU64;

    fn options(queue_capacity: usize) -> Limits {
        Limits {
            file_bytes: 320,
            files_per_tab: 2,
            total_bytes: 640,
            retention: Duration::from_secs(14 * 24 * 60 * 60),
            queue_capacity,
        }
    }

    fn now_clock() -> Clock {
        Arc::new(SystemTime::now)
    }

    fn collect_files(root: &Path) -> Vec<PathBuf> {
        let Ok(tabs) = fs::read_dir(root) else {
            return Vec::new();
        };
        tabs.flatten()
            .flat_map(|tab| fs::read_dir(tab.path()).into_iter().flatten().flatten())
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "ndjson")
            })
            .collect()
    }

    // raw hook traffic can contain environment credentials or bearer values.
    #[test]
    fn writes_redacted_hook_posts_as_ndjson() {
        let temp = tempfile::tempdir().unwrap();
        let recorder =
            ProtocolRecorder::start(temp.path().to_path_buf(), options(4), now_clock(), None);
        let body = json!({
            "CODETERM_API_TOKEN": "credential-value",
            "message": "Authorization: Bearer bearer-value"
        });
        assert!(recorder.try_record_hook_post(true, "tab-a", "stop", &body));
        recorder.barrier();
        let files = collect_files(temp.path());
        assert_eq!(files.len(), 1);
        let contents = fs::read_to_string(&files[0]).unwrap();
        assert!(!contents.contains("credential-value"));
        assert!(!contents.contains("bearer-value"));
        let value: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
        assert_eq!(value["body"]["CODETERM_API_TOKEN"], "<redacted>");
        assert_eq!(value["body"]["message"], "Authorization: Bearer <redacted>");
    }

    // hook responses must never wait behind disk writes.
    #[test]
    fn full_channel_drops_without_waiting_and_increments_counter() {
        let temp = tempfile::tempdir().unwrap();
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let recorder = ProtocolRecorder::start(
            temp.path().to_path_buf(),
            options(1),
            now_clock(),
            Some((entered_tx, release_rx)),
        );
        let body = json!({"message": "bounded"});
        assert!(recorder.try_record_hook_post(true, "tab-a", "stop", &body));
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(recorder.try_record_hook_post(true, "tab-a", "stop", &body));
        assert!(!recorder.try_record_hook_post(true, "tab-a", "stop", &body));
        assert_eq!(recorder.dropped_count(), 1);
        release_tx.send(()).unwrap();
        recorder.barrier();
        assert_eq!(collect_files(temp.path()).len(), 1);
    }

    // protocol diagnostics are opt in and disabled by default.
    #[test]
    fn disabled_recorder_creates_no_files() {
        let disabled_dir = tempfile::tempdir().unwrap();
        let recorder = ProtocolRecorder::new(disabled_dir.path().join("logs/protocol"), false);
        assert!(!recorder.try_record_hook_post(true, "tab-a", "stop", &json!({})));
        assert!(!disabled_dir.path().join("logs/protocol").exists());
    }

    // a long-running development session must not grow its protocol log without bound.
    #[test]
    fn rotation_caps_per_tab_file_count_and_size() {
        let temp = tempfile::tempdir().unwrap();
        let mut limits = options(8);
        limits.file_bytes = 320;
        limits.files_per_tab = 2;
        limits.total_bytes = 640;
        let recorder =
            ProtocolRecorder::start(temp.path().to_path_buf(), limits, now_clock(), None);
        let body = json!({"message": "x".repeat(140)});
        for _ in 0..4 {
            assert!(recorder.try_record_hook_post(true, "tab-a", "stop", &body));
            recorder.barrier();
        }
        let files = collect_files(temp.path());
        assert!(files.len() <= 2);
        assert!(files
            .iter()
            .all(|path| fs::metadata(path).unwrap().len() <= 320));
        let total = files
            .iter()
            .map(|path| fs::metadata(path).unwrap().len())
            .sum::<u64>();
        assert!(total <= 640);
    }

    // the recorder must enforce its history window without waiting for a restart.
    #[test]
    fn retention_expires_files_without_restart() {
        let temp = tempfile::tempdir().unwrap();
        let millis = Arc::new(AtomicU64::new(0));
        let clock_millis = millis.clone();
        let clock: Clock = Arc::new(move || {
            UNIX_EPOCH + Duration::from_millis(clock_millis.load(Ordering::Relaxed))
        });
        let recorder = ProtocolRecorder::start(temp.path().to_path_buf(), options(4), clock, None);
        let body = json!({"message": "retained"});
        assert!(recorder.try_record_hook_post(true, "tab-a", "stop", &body));
        recorder.barrier();
        millis.store(15 * 24 * 60 * 60 * 1000, Ordering::Relaxed);
        assert!(recorder.try_record_hook_post(true, "tab-a", "stop", &body));
        recorder.barrier();
        let files = collect_files(temp.path());
        assert_eq!(files.len(), 1);
        assert!(fs::metadata(&files[0]).unwrap().len() <= 320);
    }

    // independent per-tab rotation must not let aggregate storage exceed its global cap.
    #[test]
    fn global_total_limit_prunes_oldest_tab_file() {
        let temp = tempfile::tempdir().unwrap();
        let mut limits = options(4);
        limits.file_bytes = 320;
        limits.total_bytes = 280;
        let recorder =
            ProtocolRecorder::start(temp.path().to_path_buf(), limits, now_clock(), None);
        let body = json!({"message": "x".repeat(140)});
        assert!(recorder.try_record_hook_post(true, "tab-a", "stop", &body));
        recorder.barrier();
        assert!(recorder.try_record_hook_post(true, "tab-b", "stop", &body));
        recorder.barrier();

        let files = collect_files(temp.path());
        assert_eq!(files.len(), 1);
        assert!(files[0].to_string_lossy().contains(&tab_directory("tab-b")));
        assert!(fs::metadata(&files[0]).unwrap().len() <= 280);
    }
}
