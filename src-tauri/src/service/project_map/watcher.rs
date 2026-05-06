use super::eligibility::ext_is_indexable;
use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind, Debouncer};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct WorkspaceWatcher {
    _debouncer: Debouncer<notify::RecommendedWatcher>,
}

pub fn start_watcher<F>(
    cwd: &Path,
    debounce: Duration,
    callback: F,
) -> Result<WorkspaceWatcher, String>
where
    F: Fn() + Send + 'static,
{
    let mut debouncer = new_debouncer(debounce, move |events: notify_debouncer_mini::DebounceEventResult| {
        let Ok(events) = events else { return };
        let triggered = events.iter().any(|e| {
            matches!(e.kind, DebouncedEventKind::Any | DebouncedEventKind::AnyContinuous)
                && is_interesting_path(&e.path)
        });
        if triggered {
            callback();
        }
    })
    .map_err(|e| format!("debouncer init: {}", e))?;
    debouncer
        .watcher()
        .watch(cwd, RecursiveMode::Recursive)
        .map_err(|e| format!("watch {}: {}", cwd.display(), e))?;
    Ok(WorkspaceWatcher { _debouncer: debouncer })
}

fn is_interesting_path(p: &PathBuf) -> bool {
    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
    if !ext_is_indexable(ext) {
        return false;
    }
    let s = p.to_string_lossy();
    const SKIP_DIRS: &[&str] = &[
        "/.codeterm/", "/target/", "/node_modules/", "/dist/", "/build/",
        "/.git/", "/.venv/", "/venv/", "/__pycache__/", "/.next/", "/.turbo/",
    ];
    !SKIP_DIRS.iter().any(|d| s.contains(d))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn interesting_path_filter() {
        assert!(is_interesting_path(&PathBuf::from("/repo/src/lib.rs")));
        assert!(!is_interesting_path(&PathBuf::from("/repo/target/debug/junk.rs")));
        assert!(!is_interesting_path(&PathBuf::from("/repo/src/lib.md")));
        assert!(is_interesting_path(&PathBuf::from("/repo/src/Component.tsx")));
    }

    #[test]
    fn debouncer_fires_on_indexable_save() {
        let tmp = tempfile::tempdir().unwrap();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_in = counter.clone();
        let _watcher = start_watcher(tmp.path(), Duration::from_millis(80), move || {
            counter_in.fetch_add(1, Ordering::Relaxed);
        })
        .expect("start watcher");

        std::fs::write(tmp.path().join("a.rs"), "fn x() {}").unwrap();
        std::fs::write(tmp.path().join("b.md"), "ignore me").unwrap();
        std::thread::sleep(Duration::from_millis(400));

        let n = counter.load(Ordering::Relaxed);
        assert!(n >= 1, "expected at least one trigger, got {n}");
    }
}
