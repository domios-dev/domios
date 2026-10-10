use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::SyncSender;

pub(super) fn affects_source(event: &Event, source: &Path) -> bool {
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) && event
        .paths
        .iter()
        .any(|path| path == source || source.starts_with(path))
}

pub(super) fn watch(source: &Path, wake: SyncSender<()>) -> Result<RecommendedWatcher, String> {
    let root = source
        .ancestors()
        .skip(1)
        .find(|path| path.is_dir())
        .ok_or_else(|| format!("no watchable ancestor for {}", source.display()))?;
    let relative = source
        .strip_prefix(root)
        .map_err(|error| error.to_string())?;
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let source = root.join(relative);
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
        if event.is_ok_and(|event| affects_source(&event, &source)) {
            let _ = wake.try_send(());
        }
    })
    .map_err(|error| error.to_string())?;
    watcher
        .watch(&root, RecursiveMode::Recursive)
        .map_err(|error| error.to_string())?;
    Ok(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_materialization_wake_only_the_claimed_source() {
        let source = Path::new("sessions/own/turn.jsonl");
        assert!(affects_source(
            &Event::new(EventKind::Modify(notify::event::ModifyKind::Any)).add_path(source.into()),
            source
        ));
        assert!(affects_source(
            &Event::new(EventKind::Create(notify::event::CreateKind::Folder))
                .add_path("sessions/own".into()),
            source
        ));
        assert!(!affects_source(
            &Event::new(EventKind::Modify(notify::event::ModifyKind::Any))
                .add_path("sessions/foreign/turn.jsonl".into()),
            source
        ));
        assert!(!affects_source(
            &Event::new(EventKind::Access(notify::event::AccessKind::Any)).add_path(source.into()),
            source
        ));
    }

    #[test]
    #[cfg(unix)]
    fn filesystem_append_wakes_through_a_directory_alias() {
        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("real");
        let alias = temp.path().join("alias");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let source = alias.join("own.jsonl");
        std::fs::write(&source, b"").unwrap();
        let (wake, events) = std::sync::mpsc::sync_channel(1);
        let _watcher = watch(&source, wake).unwrap();
        std::fs::write(&source, b"opaque-turn\n").unwrap();
        events
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
    }

    #[test]
    fn filesystem_append_wakes_without_a_polling_clock() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("own.jsonl");
        std::fs::write(&source, b"").unwrap();
        let (wake, events) = std::sync::mpsc::sync_channel(1);
        let _watcher = watch(&source, wake).unwrap();
        std::fs::write(&source, b"opaque-turn\n").unwrap();
        events
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
    }
}
