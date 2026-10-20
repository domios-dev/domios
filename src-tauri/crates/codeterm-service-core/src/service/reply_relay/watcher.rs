use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};

pub(super) fn watch(
    history: &Path,
) -> Result<(RecommendedWatcher, SyncSender<()>, Receiver<()>), String> {
    let sidecar = history.with_file_name(format!(
        "{}.domios-turns.jsonl",
        history
            .file_name()
            .ok_or("history has no file name")?
            .to_string_lossy()
    ));
    let root = history
        .parent()
        .into_iter()
        .flat_map(Path::ancestors)
        .find(|path| path.is_dir())
        .ok_or("history has no watchable directory")?;
    let mode = if Some(root) == history.parent() {
        RecursiveMode::NonRecursive
    } else {
        RecursiveMode::Recursive
    };
    let (tx, rx) = sync_channel(1);
    let signal = tx.clone();
    let history = history.to_path_buf();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
        let Ok(event) = event else { return };
        if relevant_event(&event, &history, &sidecar) {
            let _ = signal.try_send(());
        }
    })
    .map_err(|error| format!("reply relay watcher: {error}"))?;
    watcher
        .watch(root, mode)
        .map_err(|error| format!("reply relay watch: {error}"))?;
    Ok((watcher, tx, rx))
}

pub(super) fn relevant_event(event: &Event, history: &Path, sidecar: &Path) -> bool {
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) && event
        .paths
        .iter()
        .any(|path| path == history || path == sidecar || history.starts_with(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn watches_exact_history_and_sidecar_without_unrelated_churn() {
        let history = Path::new("/work/own.history");
        let sidecar = Path::new("/work/own.history.domios-turns.jsonl");
        for path in [history, sidecar] {
            assert!(relevant_event(
                &Event::new(EventKind::Create(notify::event::CreateKind::File))
                    .add_path(path.into()),
                history,
                sidecar
            ));
        }
        assert!(!relevant_event(
            &Event::new(EventKind::Create(notify::event::CreateKind::File))
                .add_path("/work/other.history".into()),
            history,
            sidecar
        ));
        assert!(!relevant_event(
            &Event::new(EventKind::Access(notify::event::AccessKind::Any)).add_path(history.into()),
            history,
            sidecar
        ));
    }
}
