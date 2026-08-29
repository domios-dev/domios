use std::sync::{mpsc, Arc};
use std::time::Duration;

use crate::lock_hold::TimedRwLock;
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::state::MultiplexerState;

use super::workspace_cwds;

#[test]
fn workspace_snapshot_releases_mux_before_plan_loading() {
    let (mut state, _) = MultiplexerState::new();
    state.create_workspace("Restored".into(), Some("opaque-project".into()));
    let mux: MuxState = Arc::new(TimedRwLock::new("startup-plan-restore-test", state));

    assert_eq!(workspace_cwds(&mux), vec!["opaque-project"]);

    let writer_mux = mux.clone();
    let (writer_acquired_tx, writer_acquired_rx) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        let _guard = writer_mux.write();
        writer_acquired_tx.send(()).unwrap();
    });

    writer_acquired_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("workspace snapshot kept the mux read guard alive");
    writer.join().unwrap();
}
