use super::*;
use crate::stream_plan::{self, StreamSpec};
use std::sync::{mpsc, Mutex};
use std::time::Duration;

#[test]
fn native_exclusions_deliver_clean_marker_without_heavy_churn() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().canonicalize().unwrap();
    let excluded = vec![
        root.join("target"),
        root.join("packages/example/node_modules"),
    ];
    for path in &excluded {
        std::fs::create_dir_all(path).unwrap();
    }
    std::fs::create_dir(root.join("src")).unwrap();
    let settled = root.join("src/settled.rs");
    let marker = root.join("src/marker.rs");
    let awaited = [settled.clone(), marker.clone()];
    let observed = Arc::new(Mutex::new(Vec::new()));
    let recorded = observed.clone();
    let (arrived, arrivals) = mpsc::channel();
    let mut port = NativePort::new(move |event| {
        let mut paths = recorded.lock().unwrap();
        for path in event.paths {
            if awaited.contains(&path) {
                let _ = arrived.send(path.clone());
            }
            paths.push(path);
        }
    });
    let stream = stream_plan::start(
        &mut port,
        &StreamSpec {
            root,
            exclusions: excluded.clone(),
        },
    )
    .unwrap();
    // Fixture setup can still arrive after a SinceNow start; event ids order it before this write.
    std::fs::write(&settled, "settled").unwrap();
    await_delivery(&arrivals, &settled);
    observed.lock().unwrap().clear();
    for path in &excluded {
        for number in 0..32 {
            std::fs::write(path.join(format!("churn-{number}")), "output").unwrap();
        }
    }
    std::fs::write(&marker, "source").unwrap();
    await_delivery(&arrivals, &marker);
    unsafe {
        fs::FSEventStreamFlushSync(stream.raw);
        dispatch_sync_f(stream.queue, std::ptr::null_mut(), barrier);
    }
    let paths = observed.lock().unwrap();
    assert!(paths.contains(&marker));
    assert!(
        paths
            .iter()
            .all(|path| !excluded.iter().any(|ignored| path.starts_with(ignored))),
        "native callback received excluded churn: {paths:?}"
    );
}

fn await_delivery(arrivals: &mpsc::Receiver<PathBuf>, expected: &Path) {
    while arrivals
        .recv_timeout(Duration::from_secs(15))
        .unwrap_or_else(|_| panic!("{expected:?} was not delivered"))
        != expected
    {}
}
