use super::{enabled, PtyFlow, SourceCursor};
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
) {
    subscribe(svc, tab, anchor, nonce, Some((flow, text.to_string())));
}

fn subscribe(
    svc: &ServiceContext,
    tab: &str,
    anchor: &SendEvidenceAnchor,
    nonce: Option<&str>,
    raw: Option<(PtyFlow, String)>,
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
        .map_or_else(|| tab.to_string(), |(flow, _)| flow.id().to_string());
    if subscriptions()
        .lock()
        .get(&key)
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
            let mut raw = raw.map(|(flow, payload)| {
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
                if let Some(raw) = raw.as_mut() {
                    if raw.observe(&svc.agent_registry, &anchor, nonce.as_deref(), &cursor) {
                        break;
                    }
                }
            }
            if raw.is_some() {
                subscriptions().lock().remove(&key);
            }
        });
}
