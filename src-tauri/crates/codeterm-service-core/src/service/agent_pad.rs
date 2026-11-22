use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, LazyLock, Weak};

static PAD_GATES: LazyLock<Mutex<std::collections::HashMap<PathBuf, Weak<Mutex<()>>>>> =
    LazyLock::new(Mutex::default);
const CRON_GENERATION_FILE: &str = ".domios-cron-generation.json";

fn pad_gate(root: &Path, tab: &str) -> Arc<Mutex<()>> {
    let mut gates = PAD_GATES.lock();
    gates.retain(|_, gate| gate.strong_count() != 0);
    let key = root.join(tab);
    if let Some(gate) = gates.get(&key).and_then(Weak::upgrade) {
        return gate;
    }
    let gate = Arc::new(Mutex::new(()));
    gates.insert(key, Arc::downgrade(&gate));
    gate
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CronPadLease {
    root: PathBuf,
    tab: String,
    nonce: String,
    generation: String,
    created: bool,
}

#[derive(Serialize, Deserialize)]
struct PadGeneration {
    nonce: String,
    generation: String,
}

impl CronPadLease {
    pub(crate) fn generation(&self) -> &str {
        &self.generation
    }
    pub(crate) fn nonce(&self) -> &str {
        &self.nonce
    }
}

fn read_generation(path: &Path) -> Result<Option<PadGeneration>, String> {
    use std::io::Read;
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(4097)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 4096 {
        return Err("Cron pad generation marker exceeds its bound".into());
    }
    let record: PadGeneration = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Cron pad generation marker is invalid: {error}"))?;
    if record.nonce.len() > 512 || uuid::Uuid::parse_str(&record.generation).is_err() {
        return Err("Cron pad generation marker identity is invalid".into());
    }
    Ok(Some(record))
}

fn ensure_generation(pad: &AgentPad, nonce: &str) -> Result<CronPadLease, String> {
    if nonce.len() > 512 {
        return Err("Cron pad nonce exceeds its bound".into());
    }
    let marker = pad.path.join(CRON_GENERATION_FILE);
    let record = match read_generation(&marker)? {
        Some(record) if record.nonce == nonce => {
            return Ok(CronPadLease {
                root: pad.root.clone(),
                tab: pad.tab_id.clone(),
                nonce: record.nonce,
                generation: record.generation,
                created: false,
            })
        }
        _ => PadGeneration {
            nonce: nonce.into(),
            generation: uuid::Uuid::new_v4().to_string(),
        },
    };
    crate::persisted_state::StateFile::new(&marker)
        .with_private_permissions()
        .without_backups()
        .save(&serde_json::to_string(&record).map_err(|error| error.to_string())?)?;
    Ok(CronPadLease {
        root: pad.root.clone(),
        tab: pad.tab_id.clone(),
        nonce: record.nonce,
        generation: record.generation,
        created: false,
    })
}

const TOMBSTONE_PREFIX: &str = ".revoked-";
const POINTER_TEMPLATE: &str = "Read and follow the message in {path}";
const INBOX_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

pub use codeterm_types::agent::delivery::ChatSendResult;

pub fn chat_send_result(
    status: crate::service::chat_send_transaction::ChatSendStatus,
    pad_file: Option<PathBuf>,
) -> ChatSendResult {
    ChatSendResult {
        status,
        pad_file: pad_file.map(|path| path.display().to_string()),
    }
}

fn substitute_body_with_limit(
    body: String,
    delivered_bytes: usize,
    limit: Option<usize>,
    write_body: impl FnOnce(&[u8]) -> Result<PathBuf, String>,
) -> Result<(String, Option<PathBuf>), String> {
    let Some(limit) = limit else {
        return Ok((body, None));
    };
    if delivered_bytes <= limit {
        return Ok((body, None));
    }
    let path = write_body(body.as_bytes())?;
    let path_text = path
        .to_str()
        .ok_or_else(|| "AgentPad inbox path is not valid Unicode".to_string())?;
    Ok((POINTER_TEMPLATE.replace("{path}", path_text), Some(path)))
}

fn prune_inbox(inbox: &Path) -> Result<(), String> {
    let cutoff = std::time::SystemTime::now() - INBOX_MAX_AGE;
    for entry in fs::read_dir(inbox).map_err(|e| format!("read AgentPad inbox: {e}"))? {
        let entry = entry.map_err(|e| format!("read AgentPad inbox entry: {e}"))?;
        if entry
            .file_type()
            .map_err(|e| format!("inspect AgentPad inbox entry: {e}"))?
            .is_file()
            && entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .is_ok_and(|modified| modified < cutoff)
        {
            fs::remove_file(entry.path()).map_err(|e| format!("prune AgentPad inbox file: {e}"))?;
            if let Some(pad) = inbox.parent() {
                super::local_preview::model_copies::retire(pad, &entry.path())?;
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentPadRole {
    General,
    Orchestrator,
    Planner,
    Worker,
}

/// The one mapping from a tab's role kind to its pad identity. Restore and
/// spawn both go through here so a restarted tab reopens the SAME pad.
pub fn pad_role_for_tab(tab: &crate::multiplexer::types::Tab) -> AgentPadRole {
    use codeterm_types::agent::role_kind::builtins;
    match (tab.role, tab.role_kind_id()) {
        (Some(crate::multiplexer::types::AgentRole::Worker), _) => AgentPadRole::Worker,
        (Some(_), Some(builtins::ORCHESTRATOR)) => AgentPadRole::Orchestrator,
        (Some(_), Some(builtins::GENERAL_AGENT)) => AgentPadRole::General,
        (Some(_), _) => AgentPadRole::Planner,
        (None, _) => AgentPadRole::General,
    }
}

#[derive(Clone, Debug)]
pub struct AgentPad {
    root: PathBuf,
    tab_id: String,
    path: PathBuf,
}

impl AgentPad {
    pub fn create(data_dir: &Path, tab_id: &str, role: AgentPadRole) -> Result<Self, String> {
        validate_tab_id(tab_id)?;
        let root = data_dir.join("runtime").join("agent-pads");
        fs::create_dir_all(&root).map_err(|e| format!("create AgentPad root: {e}"))?;
        let path = root.join(tab_id);
        if path.exists() {
            return Err(format!("AgentPad already exists for tab {tab_id}"));
        }
        fs::create_dir(&path).map_err(|e| format!("create AgentPad for tab {tab_id}: {e}"))?;
        let pad = Self {
            root,
            tab_id: tab_id.to_string(),
            path,
        };
        if let Err(error) = pad.seed(role) {
            let _ = fs::remove_dir_all(&pad.path);
            return Err(error);
        }
        Ok(pad)
    }

    pub fn open(data_dir: &Path, tab_id: &str) -> Result<Self, String> {
        validate_tab_id(tab_id)?;
        let root = data_dir.join("runtime").join("agent-pads");
        let path = root.join(tab_id);
        if !path.is_dir() {
            return Err(format!("AgentPad is unavailable for tab {tab_id}"));
        }
        verify_exact_child(&root, tab_id, &path)?;
        Ok(Self {
            root,
            tab_id: tab_id.to_string(),
            path,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn open_file(&self, relative: &str) -> Result<fs::File, String> {
        validate_relative_file_path(Path::new(relative))?;
        if relative.contains(['\\', ':'])
            || relative.chars().any(char::is_control)
            || relative
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(
                "AgentPad attachment requires a relative file path without traversal".into(),
            );
        }
        codeterm_blueprint_types::code_source::open_file(&self.path, relative).map_err(|_| {
            "AgentPad attachment must be a regular file without symlinks or escapes".into()
        })
    }

    pub fn write_inbox_body(&self, body: &[u8]) -> Result<PathBuf, String> {
        let pad = self.clone();
        let body = body.to_vec();
        let write = move || pad.write_inbox_body_inner(&body);
        match tokio::runtime::Handle::try_current()
            .ok()
            .filter(|handle| handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread)
        {
            Some(handle) => tokio::task::block_in_place(|| {
                handle
                    .block_on(tokio::task::spawn_blocking(write))
                    .map_err(|e| format!("AgentPad inbox writer failed: {e}"))?
            }),
            None => write(),
        }
    }

    pub(super) fn inbox_directory(&self) -> Result<PathBuf, String> {
        let inbox = self.path.join("inbox");
        fs::create_dir_all(&inbox).map_err(|e| format!("create AgentPad inbox: {e}"))?;
        let root = self
            .path
            .canonicalize()
            .map_err(|e| format!("canonicalize AgentPad: {e}"))?;
        let inbox = inbox
            .canonicalize()
            .map_err(|e| format!("canonicalize AgentPad inbox: {e}"))?;
        if !inbox.starts_with(&root) || inbox == root {
            return Err("AgentPad inbox escapes the current tab pad".into());
        }
        Ok(inbox)
    }

    pub(crate) fn write_delivery_receipt(
        &self,
        id: &str,
        body: &[u8],
    ) -> Result<(PathBuf, bool), String> {
        uuid::Uuid::parse_str(id).map_err(|_| "invalid delivery receipt id")?;
        let inbox = self.inbox_directory()?;
        let path = inbox.join(format!("delivery-{id}.json"));
        let mut temporary =
            tempfile::NamedTempFile::new_in(&inbox).map_err(|error| error.to_string())?;
        temporary
            .write_all(body)
            .map_err(|error| error.to_string())?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| error.to_string())?;
        let created = match temporary.persist_noclobber(&path) {
            Ok(_) => true,
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
                if !metadata.file_type().is_file()
                    || metadata.len() != body.len() as u64
                    || fs::read(&path).map_err(|error| error.to_string())? != body
                {
                    log::error!(
                        "[agent-delivery] retaining edited delivery receipt: {}",
                        path.display()
                    );
                }
                false
            }
            Err(error) => return Err(error.to_string()),
        };
        #[cfg(unix)]
        fs::File::open(&inbox)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| error.to_string())?;
        Ok((
            self.path.join("inbox").join(format!("delivery-{id}.json")),
            created,
        ))
    }

    fn write_inbox_body_inner(&self, body: &[u8]) -> Result<PathBuf, String> {
        let inbox = self.inbox_directory()?;
        prune_inbox(&inbox)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| format!("read AgentPad inbox clock: {e}"))?;
        let name = format!("{}-{}.md", now.as_millis(), uuid::Uuid::new_v4());
        let path = inbox.join(&name);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| format!("create AgentPad inbox body: {e}"))?;
        file.write_all(body)
            .map_err(|e| format!("write AgentPad inbox body: {e}"))?;
        // canonicalize() adds a Windows \?\ prefix that agent file tools mishandle.
        Ok(self.path.join("inbox").join(name))
    }

    pub fn env_value(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }

    pub fn list(&self) -> Result<Vec<PathBuf>, String> {
        let canonical_root = self
            .path
            .canonicalize()
            .map_err(|e| format!("canonicalize AgentPad: {e}"))?;
        let mut files = Vec::new();
        collect_regular_files(&canonical_root, &canonical_root, &mut files)?;
        files.sort();
        Ok(files)
    }

    pub fn read_utf8(&self, relative: &Path) -> Result<String, String> {
        validate_relative_file_path(relative)?;
        let canonical_root = self
            .path
            .canonicalize()
            .map_err(|e| format!("canonicalize AgentPad: {e}"))?;
        let requested = canonical_root.join(relative);
        let canonical_file = requested
            .canonicalize()
            .map_err(|e| format!("resolve AgentPad file {}: {e}", relative.display()))?;
        if !canonical_file.starts_with(&canonical_root) || canonical_file == canonical_root {
            return Err("AgentPad path escapes the current tab pad".into());
        }
        let metadata =
            fs::metadata(&canonical_file).map_err(|e| format!("inspect AgentPad file: {e}"))?;
        if !metadata.is_file() {
            return Err("AgentPad source must be a regular file".into());
        }
        fs::read_to_string(&canonical_file).map_err(|e| format!("read AgentPad UTF-8 file: {e}"))
    }

    pub fn revoke(self) -> Result<(), String> {
        revoke(&self.root, &self.tab_id)
    }

    fn seed(&self, role: AgentPadRole) -> Result<(), String> {
        let mut names = vec!["message.md", "notes.md"];
        if role == AgentPadRole::Worker {
            names.push("report.md");
        }
        if matches!(role, AgentPadRole::Orchestrator | AgentPadRole::Planner) {
            names.push("dispatch.md");
        }
        for name in names {
            fs::write(self.path.join(name), b"")
                .map_err(|e| format!("seed AgentPad {name}: {e}"))?;
        }
        Ok(())
    }
}

pub fn restore(data_dir: &Path, tab_id: &str, role: AgentPadRole) -> Result<AgentPad, String> {
    let root = data_dir.join("runtime").join("agent-pads");
    let gate = pad_gate(&root, tab_id);
    let _guard = gate.lock();
    restore_unlocked(data_dir, tab_id, role)
}

fn restore_unlocked(data_dir: &Path, tab_id: &str, role: AgentPadRole) -> Result<AgentPad, String> {
    AgentPad::open(data_dir, tab_id).or_else(|_| AgentPad::create(data_dir, tab_id, role))
}

impl super::ServiceContext {
    pub fn prepare_chat_send_text(
        &self,
        tab_id: &str,
        text: String,
        run_stages: bool,
    ) -> Result<(String, Option<PathBuf>), String> {
        self.prepare_chat_send_text_for_send(
            tab_id,
            text,
            run_stages,
            &uuid::Uuid::new_v4().to_string(),
        )
    }

    pub(crate) fn prepare_chat_send_text_for_send(
        &self,
        tab_id: &str,
        text: String,
        run_stages: bool,
        send_id: &str,
    ) -> Result<(String, Option<PathBuf>), String> {
        let authority = super::local_preview::model_copies::capture(self, tab_id, send_id)?;
        let text = if run_stages {
            self.run_outbound_stages(tab_id, text)
        } else {
            text
        };
        let text = super::image_attachments::prepare(
            text,
            || self.ensure_agent_pad(tab_id)?.inbox_directory(),
            |path| super::local_preview::model_copies::record(self, &authority, path),
        )?;
        self.substitute_paste_limited_body(tab_id, text.clone(), text.len())
    }

    pub fn ensure_agent_pad(&self, tab_id: &str) -> Result<AgentPad, String> {
        let root = self.agent_pad_data_dir.join("runtime").join("agent-pads");
        let gate = pad_gate(&root, tab_id);
        let _guard = gate.lock();
        let (role, nonce) = self
            .mux
            .read()
            .get_tab(tab_id)
            .map(|tab| (pad_role_for_tab(tab), tab.session_binding_nonce.clone()))
            .unwrap_or((AgentPadRole::General, None));
        let pad = restore_unlocked(&self.agent_pad_data_dir, tab_id, role)?;
        if pad.path.join(CRON_GENERATION_FILE).exists() {
            if let Some(nonce) = nonce {
                ensure_generation(&pad, &nonce)?;
            }
        }
        Ok(pad)
    }

    pub(crate) fn capture_cron_pad(
        &self,
        owner: &crate::agent::run_watcher::runtime_runs::CronRunAuthority,
    ) -> Result<CronPadLease, String> {
        let tab = owner.tab_id();
        let nonce = owner
            .nonce()
            .ok_or("Cron provisional pad has no original nonce")?;
        let root = self.agent_pad_data_dir.join("runtime").join("agent-pads");
        let gate = pad_gate(&root, tab);
        let _guard = gate.lock();
        let (role, current) = self
            .mux
            .read()
            .get_tab(tab)
            .map(|tab| (pad_role_for_tab(tab), tab.session_binding_nonce.clone()))
            .ok_or("Cron provisional tab disappeared before pad capture")?;
        if current.as_deref() != Some(nonce) {
            return Err("Cron provisional generation changed before pad capture".into());
        }
        let path = root.join(tab);
        let created = !path.exists();
        let marker = path.join(CRON_GENERATION_FILE);
        let record = match read_generation(&marker)? {
            Some(record) if record.nonce == nonce => record,
            _ => PadGeneration {
                nonce: nonce.into(),
                generation: uuid::Uuid::new_v4().to_string(),
            },
        };
        let lease = CronPadLease {
            root,
            tab: tab.into(),
            nonce: nonce.into(),
            generation: record.generation.clone(),
            created,
        };
        self.run_watcher.attach_cron_pad(owner, lease.clone())?;
        self.run_watcher.journal_cron_owner(owner)?;
        restore_unlocked(&self.agent_pad_data_dir, tab, role)?;
        crate::persisted_state::StateFile::new(&marker)
            .with_private_permissions()
            .without_backups()
            .save(&serde_json::to_string(&record).map_err(|error| error.to_string())?)?;
        Ok(lease)
    }

    pub(crate) fn restore_cron_pad_lease(
        &self,
        tab: &str,
        nonce: &str,
        generation: &str,
    ) -> Result<Option<CronPadLease>, String> {
        validate_tab_id(tab)?;
        let root = self.agent_pad_data_dir.join("runtime").join("agent-pads");
        let gate = pad_gate(&root, tab);
        let _guard = gate.lock();
        let record = read_generation(&root.join(tab).join(CRON_GENERATION_FILE))?;
        Ok(record
            .filter(|record| record.nonce == nonce && record.generation == generation)
            .map(|record| CronPadLease {
                root,
                tab: tab.into(),
                nonce: record.nonce,
                generation: record.generation,
                created: false,
            }))
    }

    pub(crate) fn revoke_cron_pad(&self, lease: &CronPadLease) -> Result<(), String> {
        validate_tab_id(&lease.tab)?;
        let root = self.agent_pad_data_dir.join("runtime").join("agent-pads");
        if lease.root != root {
            return Err("Cron pad lease belongs to another root".into());
        }
        let gate = pad_gate(&root, &lease.tab);
        let guard = gate.lock();
        let path = root.join(&lease.tab);
        let tombstone = root.join(format!(
            "{TOMBSTONE_PREFIX}{}-{}",
            lease.tab, lease.generation
        ));
        if path.exists() {
            let marker = read_generation(&path.join(CRON_GENERATION_FILE))?;
            if marker.as_ref().is_some_and(|marker| {
                marker.nonce != lease.nonce || marker.generation != lease.generation
            }) {
                return Ok(());
            }
            if marker.is_none() && !lease.created {
                return Err("Original cron pad marker is absent; cleanup remains owned".into());
            }
            verify_exact_child(&root, &lease.tab, &path)?;
            let mux = self.mux.read();
            if mux.get_tab(&lease.tab).is_some_and(|tab| {
                tab.session_binding_nonce.as_deref() != Some(lease.nonce.as_str())
            }) {
                return Ok(());
            }
            if marker.is_none() && mux.get_tab(&lease.tab).is_none() {
                return Err("Unmarked provisional pad has no current original generation; preserve for adjudication".into());
            }
            fs::rename(&path, &tombstone)
                .map_err(|error| format!("Claim original cron pad teardown: {error}"))?;
            drop(mux);
        }
        drop(guard);
        if tombstone.exists() {
            let actual_root = root.canonicalize().map_err(|error| error.to_string())?;
            let actual = tombstone
                .canonicalize()
                .map_err(|error| error.to_string())?;
            if actual.parent() != Some(actual_root.as_path()) {
                return Err("Cron pad teardown escaped its root".into());
            }
            fs::remove_dir_all(&actual)
                .map_err(|error| format!("Retire original cron pad: {error}"))?;
        }
        Ok(())
    }

    pub fn substitute_paste_limited_body(
        &self,
        tab_id: &str,
        body: String,
        delivered_bytes: usize,
    ) -> Result<(String, Option<PathBuf>), String> {
        let provider_id = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.agent_type.clone());
        let limit = provider_id
            .as_deref()
            .and_then(|id| {
                self.agent_registry
                    .with_provider(id, |provider| provider.paste_limit_bytes())
            })
            .flatten();
        substitute_body_with_limit(body, delivered_bytes, limit, |bytes| {
            self.ensure_agent_pad(tab_id)?.write_inbox_body(bytes)
        })
    }

    pub fn revoke_agent_pad(&self, tab_id: &str) -> Result<(), String> {
        revoke(
            &self.agent_pad_data_dir.join("runtime").join("agent-pads"),
            tab_id,
        )
    }
}

pub fn revoke(root: &Path, tab_id: &str) -> Result<(), String> {
    let gate = pad_gate(root, tab_id);
    let _guard = gate.lock();
    revoke_unlocked(root, tab_id)
}

fn revoke_unlocked(root: &Path, tab_id: &str) -> Result<(), String> {
    validate_tab_id(tab_id)?;
    let path = root.join(tab_id);
    if !path.exists() {
        return Ok(());
    }
    verify_exact_child(root, tab_id, &path)?;
    match fs::remove_dir_all(&path) {
        Ok(()) => Ok(()),
        Err(remove_error) => {
            let tombstone = root.join(format!(
                "{TOMBSTONE_PREFIX}{tab_id}-{}",
                uuid::Uuid::new_v4()
            ));
            fs::rename(&path, &tombstone).map_err(|rename_error| {
                format!("revoke AgentPad: {remove_error}; tombstone rename: {rename_error}")
            })
        }
    }
}

pub fn reconcile(data_dir: &Path, live_tab_ids: &HashSet<String>) -> Result<(), String> {
    let root = data_dir.join("runtime").join("agent-pads");
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(&root).map_err(|e| format!("read AgentPad root: {e}"))? {
        let entry = entry.map_err(|e| format!("read AgentPad entry: {e}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(TOMBSTONE_PREFIX) || !live_tab_ids.contains(&name) {
            let path = entry.path();
            if path.parent() != Some(root.as_path()) {
                return Err("invalid AgentPad reconciliation target".into());
            }
            if path.is_dir() {
                fs::remove_dir_all(path)
            } else {
                fs::remove_file(path)
            }
            .map_err(|e| format!("remove orphan AgentPad {name}: {e}"))?;
        }
    }
    Ok(())
}

fn validate_tab_id(tab_id: &str) -> Result<(), String> {
    let path = Path::new(tab_id);
    if tab_id.is_empty()
        || path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
    {
        return Err("AgentPad requires a canonical full tab id".into());
    }
    Ok(())
}

fn validate_relative_file_path(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("--from-pad must be a non-empty relative file path without traversal".into());
    }
    Ok(())
}

fn verify_exact_child(root: &Path, tab_id: &str, path: &Path) -> Result<(), String> {
    if path.parent() != Some(root) || path.file_name().and_then(|v| v.to_str()) != Some(tab_id) {
        return Err("refusing AgentPad operation outside its exact tab child".into());
    }
    Ok(())
}

fn collect_regular_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("list AgentPad: {e}"))? {
        let entry = entry.map_err(|e| format!("list AgentPad entry: {e}"))?;
        let canonical = entry
            .path()
            .canonicalize()
            .map_err(|e| format!("resolve AgentPad entry: {e}"))?;
        if !canonical.starts_with(root) {
            return Err("AgentPad entry escapes the current tab pad".into());
        }
        let metadata =
            fs::metadata(&canonical).map_err(|e| format!("inspect AgentPad entry: {e}"))?;
        if metadata.is_dir() {
            collect_regular_files(root, &canonical, out)?;
        } else if metadata.is_file() {
            let relative = canonical.strip_prefix(root).unwrap();
            if relative.to_str().is_none() {
                return Err("AgentPad file name is not UTF-8".into());
            }
            out.push(relative.to_path_buf());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
