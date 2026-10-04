use super::*;

use crate::lock_hold::TimedRwLock;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{
    TurnResumeMark, TurnResumeOutcome, TurnResumeSettlement,
};

fn resume_mark(snapshot: &SessionSnapshot, tab_id: &str) -> Option<TurnResumeMark> {
    fn in_node(
        node: &store::SplitNodeSnapshot,
        tab_id: &str,
    ) -> Option<TurnResumeMark> {
        match node {
            store::SplitNodeSnapshot::Leaf {
                tab_id: found,
                agent_identity,
                ..
            } if found == tab_id => agent_identity.as_ref()?.turn_resume.clone(),
            store::SplitNodeSnapshot::Leaf { .. } => None,
            store::SplitNodeSnapshot::Split { children, .. } => {
                children.iter().find_map(|child| in_node(child, tab_id))
            }
        }
    }

    snapshot
        .workspaces
        .iter()
        .flat_map(|workspace| &workspace.tabs)
        .find_map(|tab| in_node(&tab.split_tree, tab_id))
}

#[test]
fn snapshot_capture_waits_for_store_lock_and_sees_the_sent_claim() {
    let dir = tempfile::tempdir().unwrap();
    let session: SessionState = Arc::new(Mutex::new(SessionStore::for_test_dir(
        dir.path().to_path_buf(),
    )));
    let (mut state, tab_id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("opaque-provider".into());
    tab.agent_session_id = Some("session-a".into());
    tab.turn_resume = Some(TurnResumeMark {
        session_id: "session-a".into(),
        start_epoch: "prior-start".into(),
        prompt: None,
        sender: None,
        settled: None,
    });
    let mux: MuxState = Arc::new(TimedRwLock::new("snapshot-lock-test", state));
    let (locked_tx, locked_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let save_session = session.clone();
    let save_mux = mux.clone();

    let saver = std::thread::spawn(move || {
        save_current_snapshot_after_lock(
            &save_session,
            &save_mux,
            store::SaveIntent::PreserveDesk,
            || {
                locked_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            },
        )
    });

    locked_rx.recv().unwrap();
    {
        let mut mux = mux.write();
        let mark = &mut mux.get_tab_mut(&tab_id).unwrap().turn_resume;
        mark.as_mut().unwrap().settled = Some(TurnResumeSettlement {
            start_epoch: "current-start".into(),
            outcome: TurnResumeOutcome::Sent,
        });
    }
    release_tx.send(()).unwrap();
    saver.join().unwrap().unwrap();

    let saved = session.lock().load_last().unwrap().expect("snapshot saved");
    assert_eq!(
        resume_mark(&saved, &tab_id)
            .and_then(|mark| mark.settled)
            .map(|settlement| settlement.outcome),
        Some(TurnResumeOutcome::Sent)
    );
}
