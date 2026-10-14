use super::{enabled, PtyFlow, SourceCursor};
use crate::service::{
    chat_send_transaction::{EvidenceWindow, SendEvidenceAnchor},
    ServiceContext,
};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{mpsc, OnceLock},
};

pub(crate) fn capture_window(
    source: &crate::service::tab_chat::registry::JsonlSource,
    session: &str,
    path: &Path,
) -> Option<EvidenceWindow> {
    use crate::service::tab_chat::registry::SourceCapture;
    match source.capture(session, path) {
        SourceCapture::Present { len } => Some(EvidenceWindow::Jsonl {
            transcript_offset: len,
        }),
        SourceCapture::Absent { .. } => Some(EvidenceWindow::Jsonl {
            transcript_offset: 0,
        }),
        SourceCapture::Unreadable => None,
    }
}

fn normalized_path(path: &Path) -> PathBuf {
    codeterm_paths::strip_verbatim_prefix(&path.to_string_lossy()).into()
}

pub(crate) fn register_source(
    path: &Path,
    events: mpsc::SyncSender<()>,
) -> Result<RecommendedWatcher, &'static str> {
    let parent = path.parent().ok_or("parent_unavailable")?;
    let root = parent
        .ancestors()
        .find(|path| path.is_dir())
        .ok_or("parent_unavailable")?;
    let canonical = std::fs::canonicalize(root).map_err(|_| "parent_unreadable")?;
    let relative = path.strip_prefix(root).map_err(|_| "path_unavailable")?;
    let watched = normalized_path(&canonical).join(relative);
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok_and(|event| {
            matches!(
                event.kind,
                notify::EventKind::Create(_)
                    | notify::EventKind::Modify(_)
                    | notify::EventKind::Remove(_)
            ) && event.paths.iter().any(|path| {
                let changed = normalized_path(path);
                changed == watched || watched.starts_with(&changed)
            })
        }) {
            let _ = events.try_send(());
        }
    })
    .map_err(|_| "watcher_unavailable")?;
    let mode = if root == parent {
        RecursiveMode::NonRecursive
    } else {
        RecursiveMode::Recursive
    };
    watcher
        .watch(&normalized_path(&canonical), mode)
        .map_err(|_| "watch_failed")?;
    Ok(watcher)
}

struct Subscription {
    tab: String,
    identity: (String, Option<String>),
    _watcher: RecommendedWatcher,
}

fn subscriptions() -> &'static Mutex<HashMap<String, Subscription>> {
    static SUBSCRIPTIONS: OnceLock<Mutex<HashMap<String, Subscription>>> = OnceLock::new();
    SUBSCRIPTIONS.get_or_init(Mutex::default)
}

pub(crate) fn cancel(tab: &str) {
    let old = {
        let mut entries = subscriptions().lock();
        let keys = entries
            .iter()
            .filter(|(_, entry)| entry.tab == tab)
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        keys.into_iter()
            .filter_map(|key| entries.remove(&key))
            .collect::<Vec<_>>()
    };
    drop(old);
}

pub(crate) fn ensure(
    svc: &ServiceContext,
    tab: &str,
    anchor: &SendEvidenceAnchor,
    nonce: Option<&str>,
) {
    subscribe(svc, tab, anchor, nonce, None);
}

pub(super) fn ensure_unledgered(
    svc: &ServiceContext,
    tab: &str,
    anchor: &SendEvidenceAnchor,
    nonce: Option<&str>,
    flow: PtyFlow,
    text: &str,
    path: PathBuf,
) {
    subscribe(
        svc,
        tab,
        anchor,
        nonce,
        Some((flow, text.to_string(), path)),
    );
}

fn subscribe(
    svc: &ServiceContext,
    tab: &str,
    anchor: &SendEvidenceAnchor,
    nonce: Option<&str>,
    raw: Option<(PtyFlow, String, PathBuf)>,
) {
    if !enabled() {
        return;
    }
    let EvidenceWindow::Jsonl { transcript_offset } = &anchor.window else {
        return;
    };
    let identity = (anchor.session_id.clone(), nonce.map(str::to_string));
    let key = raw
        .as_ref()
        .map_or_else(|| tab.to_string(), |(flow, _, _)| flow.id().to_string());
    if subscriptions()
        .lock()
        .get(&key)
        .is_some_and(|entry| entry.identity == identity)
    {
        return;
    }
    let Some(path) = raw.as_ref().map(|(_, _, path)| path.clone()).or_else(|| {
        svc.agent_registry
            .with_provider(&anchor.provider_id, |provider| {
                provider.session_jsonl_path(&anchor.cwd, &anchor.session_id)
            })
            .flatten()
    }) else {
        return;
    };
    let (tx, rx) = mpsc::sync_channel(1);
    let watcher = match register_source(&path, tx.clone()) {
        Ok(watcher) => watcher,
        Err(outcome) => {
            if let Some((flow, _, _)) = &raw {
                flow.observer(outcome);
            }
            return;
        }
    };
    if let Some((flow, _, _)) = &raw {
        flow.observer(if path.exists() {
            "registered"
        } else {
            "registered_before_materialization"
        });
    }
    let old = {
        let mut entries = subscriptions().lock();
        if entries
            .get(&key)
            .is_some_and(|entry| entry.identity == identity)
        {
            return;
        }
        if entries.len() >= 256 {
            if let Some(key) = entries.keys().next().cloned() {
                entries.remove(&key);
            }
        }
        entries.insert(
            key.clone(),
            Subscription {
                tab: tab.into(),
                identity,
                _watcher: watcher,
            },
        )
    };
    drop(old);
    let _ = tx.try_send(());
    drop(tx);
    let (svc, tab, anchor, nonce) = (
        svc.clone(),
        tab.to_string(),
        anchor.clone(),
        nonce.map(str::to_string),
    );
    let mut offset = *transcript_offset;
    let _ = std::thread::Builder::new()
        .name("dev-trace-source".into())
        .spawn(move || {
            let mut cursor = SourceCursor::default();
            let mut raw = raw.map(|(flow, payload, _)| {
                super::source::UnledgeredObservation::new(flow, payload, &anchor)
            });
            while rx.recv().is_ok() {
                let current = svc
                    .mux
                    .read()
                    .get_tab(&tab)
                    .filter(|tab| {
                        tab.agent_session_id.as_deref() == Some(&anchor.session_id)
                            && tab.session_binding_nonce == nonce
                            && (raw.is_none() || tab.reported_session_binding_nonce == nonce)
                    })
                    .map(|tab| tab.fork_source_session_id.is_some());
                let Some(fork) = current else {
                    if let Some(raw) = &raw {
                        raw.observer("binding_changed");
                    }
                    break;
                };
                if !path.exists() {
                    continue;
                }
                let Some(delta) = svc
                    .agent_registry
                    .with_provider(&anchor.provider_id, |provider| {
                        provider.parse_session_delta(&path, offset)
                    })
                else {
                    if let Some(raw) = &raw {
                        raw.observer("provider_unavailable");
                    }
                    break;
                };
                offset = delta.new_offset.max(offset);
                let messages = delta
                    .messages
                    .iter()
                    .filter(|message| {
                        message
                            .provider_session_id
                            .as_deref()
                            .is_none_or(|id| id == anchor.session_id)
                            && (!fork
                                || message.provider_session_id.as_deref()
                                    == Some(&anchor.session_id))
                            && (message.role != "user"
                                || message.uuid.as_ref().is_some_and(|id| !id.is_empty())
                                || message.record_start.is_some())
                    })
                    .enumerate()
                    .flat_map(|(index, message)| {
                        crate::service::tab_chat::rich_to_chat_msgs_live(message, index)
                    })
                    .collect::<Vec<_>>();
                cursor.pty(
                    &tab,
                    &anchor.provider_id,
                    &anchor.session_id,
                    nonce.as_deref(),
                    &messages,
                );
                if let Some(raw) = raw.as_mut() {
                    if raw.observe(&anchor, nonce.as_deref(), &messages, &cursor) {
                        break;
                    }
                }
            }
            if raw.is_some() {
                subscriptions().lock().remove(&key);
            }
        });
}

#[cfg(test)]
mod tests;
