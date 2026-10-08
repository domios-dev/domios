use super::{enabled, SourceCursor};
use crate::service::{
    chat_send_transaction::{EvidenceWindow, SendEvidenceAnchor},
    ServiceContext,
};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    sync::{mpsc, OnceLock},
};

struct Subscription {
    identity: (String, Option<String>),
    _watcher: RecommendedWatcher,
}

fn subscriptions() -> &'static Mutex<HashMap<String, Subscription>> {
    static SUBSCRIPTIONS: OnceLock<Mutex<HashMap<String, Subscription>>> = OnceLock::new();
    SUBSCRIPTIONS.get_or_init(Mutex::default)
}

pub(crate) fn cancel(tab: &str) {
    let old = subscriptions().lock().remove(tab);
    drop(old);
}

pub(crate) fn ensure(
    svc: &ServiceContext,
    tab: &str,
    anchor: &SendEvidenceAnchor,
    nonce: Option<&str>,
) {
    if !enabled() {
        return;
    }
    let EvidenceWindow::Jsonl { transcript_offset } = &anchor.window else {
        return;
    };
    let identity = (anchor.session_id.clone(), nonce.map(str::to_string));
    if subscriptions()
        .lock()
        .get(tab)
        .is_some_and(|entry| entry.identity == identity)
    {
        return;
    }
    let Some(path) = svc
        .agent_registry
        .with_provider(&anchor.provider_id, |provider| {
            provider.session_jsonl_path(&anchor.cwd, &anchor.session_id)
        })
        .flatten()
    else {
        return;
    };
    let Some(parent) = path.parent().filter(|parent| parent.is_dir()) else {
        return;
    };
    let (tx, rx) = mpsc::sync_channel(1);
    let watched = path.clone();
    let events = tx.clone();
    let Ok(mut watcher) =
        notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if event.is_ok_and(|event| {
                matches!(
                    event.kind,
                    notify::EventKind::Create(_)
                        | notify::EventKind::Modify(_)
                        | notify::EventKind::Remove(_)
                ) && event.paths.iter().any(|path| path == &watched)
            }) {
                let _ = events.try_send(());
            }
        })
    else {
        return;
    };
    if watcher.watch(parent, RecursiveMode::NonRecursive).is_err() {
        return;
    }
    let old = {
        let mut entries = subscriptions().lock();
        if entries
            .get(tab)
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
            tab.into(),
            Subscription {
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
            while rx.recv().is_ok() {
                let current = svc
                    .mux
                    .read()
                    .get_tab(&tab)
                    .filter(|tab| {
                        tab.agent_session_id.as_deref() == Some(&anchor.session_id)
                            && tab.session_binding_nonce == nonce
                    })
                    .map(|tab| tab.fork_source_session_id.is_some());
                let Some(fork) = current else {
                    break;
                };
                let Some(delta) = svc
                    .agent_registry
                    .with_provider(&anchor.provider_id, |provider| {
                        provider.parse_session_delta(&path, offset)
                    })
                else {
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
            }
        });
}
