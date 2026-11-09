use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::SystemTime;

use parking_lot::Mutex;
use serde::Serialize;

use super::{references, tab_authority, TabAuthority};
use crate::service::{
    tab_chat::{
        jsonl_kernel,
        registry::{JsonlSource, SourceIncarnation, SourceObservation},
    },
    ServiceContext,
};

const MAX_ENTRIES: usize = 32;
const MAX_REFERENCE_BYTES: usize = 512 * 1024;
const CHUNK_BYTES: u64 = 1024 * 1024;
pub const INDEXING: &str = "Preview references are indexing";

#[derive(Clone, PartialEq)]
struct Source {
    authority: TabAuthority,
    provider: String,
    session: String,
    path: PathBuf,
}

#[derive(Clone, Serialize)]
pub struct Preparation {
    pub state: &'static str,
    pub index_id: String,
}

#[derive(Default)]
struct State {
    references: Arc<HashSet<String>>,
    offset: u64,
    total: u64,
    modified: Option<SystemTime>,
    incarnation: Option<SourceIncarnation>,
    running: bool,
    complete: bool,
    error: Option<String>,
    index_id: String,
}

struct Entry {
    source: Source,
    file: JsonlSource,
    cancelled: AtomicBool,
    state: Mutex<State>,
}

#[derive(Default)]
pub struct ReferenceInventory {
    entries: Mutex<HashMap<String, Arc<Entry>>>,
}

impl ReferenceInventory {
    pub fn drop_tab(&self, tab: &str) {
        if let Some(entry) = self.entries.lock().remove(tab) {
            entry.cancelled.store(true, Ordering::Release);
        }
    }

    fn entry(&self, tab: &str, source: Source) -> Result<Arc<Entry>, String> {
        let mut entries = self.entries.lock();
        if let Some(entry) = entries.get(tab) {
            if entry.source == source {
                return Ok(entry.clone());
            }
            entry.cancelled.store(true, Ordering::Release);
        }
        if !entries.contains_key(tab) && entries.len() >= MAX_ENTRIES {
            let idle = entries.iter().find_map(|(tab, entry)| {
                entry
                    .state
                    .try_lock()
                    .filter(|state| !state.running)
                    .map(|_| tab.clone())
            });
            let idle = idle.ok_or("Preview reference inventory capacity exceeded")?;
            entries.remove(&idle);
        }
        let entry = Arc::new(Entry {
            source,
            file: JsonlSource::default(),
            cancelled: AtomicBool::new(false),
            state: Mutex::new(State::default()),
        });
        entries.insert(tab.to_owned(), entry.clone());
        Ok(entry)
    }

    pub fn memory_stats(&self) -> serde_json::Value {
        let Some(entries) = self.entries.try_lock() else {
            return serde_json::json!({"busy":true});
        };
        let bytes: usize = entries
            .values()
            .filter_map(|entry| entry.state.try_lock())
            .map(|state| reference_bytes(&state.references))
            .sum();
        serde_json::json!({"entries":entries.len(), "reference_bytes":bytes,
            "max_entries":MAX_ENTRIES, "max_reference_bytes_per_entry":MAX_REFERENCE_BYTES})
    }
}

fn reference_bytes(paths: &HashSet<String>) -> usize {
    paths.capacity() * std::mem::size_of::<String>()
        + paths.iter().map(String::capacity).sum::<usize>()
}

fn source(svc: &ServiceContext, tab: &str) -> Result<Option<Source>, String> {
    let authority = tab_authority(svc, tab)?;
    if authority.backing != crate::multiplexer::types::TabBacking::Pty {
        return Ok(None);
    }
    let (Some(provider), Some(session)) = (&authority.provider, &authority.session) else {
        return Ok(None);
    };
    let path = svc
        .agent_registry
        .with_provider(provider, |provider| {
            (!provider.uses_structured_chat())
                .then(|| {
                    provider
                        .session_jsonl_path(authority.cwd.as_deref().unwrap_or_default(), session)
                })
                .flatten()
        })
        .flatten();
    let provider = provider.clone();
    let session = session.clone();
    Ok(path.map(|path| Source {
        provider,
        session,
        authority,
        path,
    }))
}

fn observe(entry: &Entry, state: &State) -> Result<(SourceIncarnation, bool), String> {
    match entry.file.observe(
        &entry.source.session,
        &entry.source.path,
        state.incarnation.as_ref(),
        state.offset,
    ) {
        SourceObservation::Present {
            incarnation: Some(incarnation),
            restart,
        } => Ok((incarnation, restart)),
        _ => Err("Preview transcript is unavailable".into()),
    }
}

fn current(entry: &Entry, state: &State) -> Result<bool, String> {
    let meta = std::fs::metadata(&entry.source.path).map_err(|error| error.to_string())?;
    let (incarnation, restart) = observe(entry, state)?;
    Ok(state.complete
        && !restart
        && state.incarnation.as_ref() == Some(&incarnation)
        && state.total == meta.len()
        && state.modified == meta.modified().ok())
}

pub fn prepare(svc: &ServiceContext, tab: &str) -> Result<Preparation, String> {
    let Some(source) = source(svc, tab)? else {
        return Ok(Preparation {
            state: "ready",
            index_id: String::new(),
        });
    };
    let entry = svc.chat_registry.preview_references.entry(tab, source)?;
    let mut state = entry.state.lock();
    if state.running {
        return Ok(Preparation {
            state: "indexing",
            index_id: state.index_id.clone(),
        });
    }
    if std::fs::metadata(&entry.source.path)
        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        *state = State::default();
        return Ok(Preparation {
            state: "ready",
            index_id: String::new(),
        });
    }
    if current(&entry, &state)? {
        return Ok(Preparation {
            state: "ready",
            index_id: state.index_id.clone(),
        });
    }
    if let Some(error) = &state.error {
        return Err(error.clone());
    }
    state.running = true;
    state.index_id = uuid::Uuid::new_v4().to_string();
    let index_id = state.index_id.clone();
    drop(state);
    let worker = entry.clone();
    let svc = svc.clone();
    let tab = tab.to_owned();
    crate::async_runtime::spawn_blocking(move || {
        let started = std::time::Instant::now();
        let result = build(&svc, &tab, &worker);
        let mut state = worker.state.lock();
        state.running = false;
        state.error = result.as_ref().err().cloned();
        state.complete = result.is_ok();
        let index_id = state.index_id.clone();
        log::debug!(
            "[preview.index] tab={} elapsed_ms={} indexed_bytes={} references={} ok={}",
            tab,
            started.elapsed().as_millis(),
            state.offset,
            state.references.len(),
            result.is_ok()
        );
        drop(state);
        svc.ui.emit(
            &format!("preview-index-state-{tab}"),
            serde_json::json!({
                "index_id":index_id, "state":if result.is_ok() {"ready"} else {"failed"}
            }),
        );
    });
    Ok(Preparation {
        state: "indexing",
        index_id,
    })
}

pub fn lookup(svc: &ServiceContext, tab: &str) -> Result<Option<Arc<HashSet<String>>>, String> {
    let Some(source) = source(svc, tab)? else {
        return Ok(None);
    };
    let entry = svc.chat_registry.preview_references.entry(tab, source)?;
    let state = entry.state.lock();
    if std::fs::metadata(&entry.source.path)
        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        return Ok(Some(Arc::default()));
    }
    if let Some(error) = &state.error {
        return Err(error.clone());
    }
    let meta = std::fs::metadata(&entry.source.path).map_err(|error| error.to_string())?;
    let (incarnation, restart) = observe(&entry, &state)?;
    if !state.complete
        || restart
        || state.incarnation.as_ref() != Some(&incarnation)
        || meta.len() < state.total
        || (meta.len() == state.total && meta.modified().ok() != state.modified)
    {
        return Err(INDEXING.into());
    }
    Ok(Some(state.references.clone()))
}

fn build(svc: &ServiceContext, tab: &str, entry: &Entry) -> Result<(), String> {
    let meta = std::fs::metadata(&entry.source.path).map_err(|error| error.to_string())?;
    let total = meta.len();
    let mut state = entry.state.lock();
    let (incarnation, restart) = observe(entry, &state)?;
    if restart
        || state.incarnation.as_ref() != Some(&incarnation)
        || total < state.total
        || (total == state.total && state.modified != meta.modified().ok())
    {
        state.references = Arc::default();
        state.offset = 0;
    }
    let mut paths = (*state.references).clone();
    let mut offset = state.offset;
    drop(state);
    while offset < total {
        if entry.cancelled.load(Ordering::Acquire)
            || entry.source.authority != tab_authority(svc, tab)?
        {
            return Err("Preview session changed".into());
        }
        let delta = svc
            .agent_registry
            .with_provider(&entry.source.provider, |provider| {
                jsonl_kernel::try_delta(
                    provider,
                    &entry.source.path,
                    offset,
                    CHUNK_BYTES.min(total - offset),
                )
            })
            .ok_or("Session provider is unavailable")??
            .ok_or("Session transcript is unavailable")?;
        if delta.new_offset <= offset {
            break;
        }
        if delta.new_offset > total {
            return Err("Preview transcript changed".into());
        }
        for (ordinal, rich) in delta.messages.iter().enumerate() {
            let rows: Vec<_> = crate::service::tab_chat::rich_to_chat_msgs_at(
                rich,
                crate::service::tab_chat::RowOrigin::at(offset, ordinal),
            )
            .into_iter()
            .map(crate::agent::chat_types::ChatMsg::with_segments)
            .collect();
            paths.extend(references::referenced_paths(
                &rows,
                entry.source.authority.cwd.as_deref(),
            ));
        }
        if reference_bytes(&paths) > MAX_REFERENCE_BYTES {
            return Err("Preview reference index capacity exceeded".into());
        }
        offset = delta.new_offset;
    }
    if entry.cancelled.load(Ordering::Acquire) || entry.source.authority != tab_authority(svc, tab)?
    {
        return Err("Preview session changed".into());
    }
    let mut state = entry.state.lock();
    let final_state = State {
        offset,
        incarnation: Some(incarnation.clone()),
        ..State::default()
    };
    let (observed, restart) = observe(entry, &final_state)?;
    if restart || observed != incarnation {
        return Err("Preview transcript changed".into());
    }
    state.references = paths.into();
    state.offset = offset;
    state.total = total;
    state.modified = meta.modified().ok();
    state.incarnation = Some(incarnation);
    Ok(())
}

#[cfg(test)]
mod tests;
