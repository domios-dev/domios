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
    let marker = root.join("src/marker.rs");
    let marker_callback = marker.clone();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let recorded = observed.clone();
    let (ready, delivered) = mpsc::channel();
    let mut port = NativePort::new(move |event| {
        let marker_seen = event.paths.contains(&marker_callback);
        recorded.lock().unwrap().extend(event.paths);
        if marker_seen {
            let _ = ready.send(());
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
    for path in &excluded {
        for number in 0..32 {
            std::fs::write(path.join(format!("churn-{number}")), "output").unwrap();
        }
    }
    std::fs::write(&marker, "source").unwrap();
    delivered
        .recv_timeout(Duration::from_secs(15))
        .expect("clean marker was not delivered");
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
