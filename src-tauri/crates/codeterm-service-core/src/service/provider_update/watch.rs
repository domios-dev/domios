use notify::{RecursiveMode, Watcher};
use std::sync::atomic::{AtomicBool, Ordering};

use super::{storage, ServiceContext};

static STARTED: AtomicBool = AtomicBool::new(false);

pub(super) fn start(service: &ServiceContext) {
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    let Some(dir) = storage::provider_update_state_dir() else {
        STARTED.store(false, Ordering::Release);
        return;
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event {
            if (event.kind.is_modify() || event.kind.is_create())
                && event.paths.iter().any(|path| {
                    path.file_name()
                        .is_some_and(|name| name == storage::STATE_FILE)
                })
            {
                storage::changed();
                let _ = tx.send(());
            }
        }
    });
    let Ok(mut watcher) = watcher else {
        STARTED.store(false, Ordering::Release);
        return;
    };
    if let Err(error) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
        STARTED.store(false, Ordering::Release);
        log::error!("[provider-update] state watcher failed: {error}");
        return;
    }
    let service = service.clone();
    let result = std::thread::Builder::new()
        .name("provider-update-state-events".into())
        .spawn(move || {
            let _watcher = watcher;
            for () in rx {
                super::idle::schedule(&service, None);
            }
        });
    if let Err(error) = result {
        STARTED.store(false, Ordering::Release);
        log::error!("[provider-update] state event utility failed: {error}");
    }
}
