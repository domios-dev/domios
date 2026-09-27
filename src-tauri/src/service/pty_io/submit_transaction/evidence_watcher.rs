use std::sync::mpsc::Sender;

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::agent::registry::AgentRegistry;
use crate::service::chat_send_transaction::{EvidenceWindow, SendEvidenceAnchor};

pub(super) fn watch_session_evidence(
    registry: &AgentRegistry,
    anchor: Option<&SendEvidenceAnchor>,
    wake_sender: Sender<()>,
) -> Option<RecommendedWatcher> {
    let anchor = anchor?;
    if !matches!(&anchor.window, EvidenceWindow::Jsonl { .. }) {
        return None;
    }
    let path = registry
        .with_provider(&anchor.provider_id, |provider| {
            provider.session_jsonl_path(&anchor.cwd, &anchor.session_id)
        })
        .flatten()?;
    let parent = path.parent()?.to_path_buf();
    let watched_parent = parent.clone();
    let watched_name = path.file_name()?.to_os_string();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
        let Ok(event) = event else { return };
        if event.paths.iter().any(|changed| {
            changed.parent() == Some(watched_parent.as_path())
                && changed.file_name() == Some(watched_name.as_os_str())
        }) {
            let _ = wake_sender.send(());
        }
    })
    .ok()?;
    watcher.watch(&parent, RecursiveMode::NonRecursive).ok()?;
    Some(watcher)
}
