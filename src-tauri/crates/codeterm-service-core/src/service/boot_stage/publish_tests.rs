use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

fn published_stage(service: &ServiceContext, tab_id: &str) -> Option<serde_json::Value> {
    let snapshot: serde_json::Value =
        serde_json::from_slice(&service.published_mux_state()).unwrap();
    snapshot["tabs"][tab_id].get("boot_stage").cloned()
}

fn observe_published_stage(
    fixture: &ServiceFixture,
    tab_id: &str,
    stage: BootStage,
) -> (usize, std::sync::mpsc::Receiver<()>) {
    let (published, observed) = std::sync::mpsc::channel();
    let tab_id = tab_id.to_owned();
    let expected = serde_json::to_value(stage).unwrap();
    let service = fixture.service.clone();
    let listener = fixture.ui.listen("mux-state-changed", move |_| {
        if published_stage(&service, &tab_id) == Some(expected.clone()) {
            let _ = published.send(());
        }
    });
    (listener, observed)
}

fn await_publication(
    fixture: &ServiceFixture,
    observation: (usize, std::sync::mpsc::Receiver<()>),
) {
    let (listener, observed) = observation;
    let result = observed.recv_timeout(std::time::Duration::from_secs(10));
    fixture.ui.unlisten(listener);
    result.expect("emitter completes publication of the committed boot stage");
}

#[test]
fn boot_stage_commit_reaches_published_snapshot_without_other_mutation() {
    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let before = fixture.service.published_mux_state();
    let observation = observe_published_stage(&fixture, &tab_id, BootStage::LaunchWritten);
    fixture
        .service
        .emit_boot_stage(&tab_id, BootStage::LaunchWritten, None);
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .boot_stage,
        Some(BootStage::LaunchWritten)
    );
    await_publication(&fixture, observation);
    assert_eq!(
        published_stage(&fixture.service, &tab_id),
        Some(serde_json::to_value(BootStage::LaunchWritten).unwrap())
    );
    assert_ne!(fixture.service.published_mux_state(), before);
}

#[test]
fn boot_stage_burst_reaches_published_snapshot_without_other_mutation() {
    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let observation = observe_published_stage(&fixture, &tab_id, BootStage::ProviderReady);
    for stage in [
        BootStage::AgentLaunching,
        BootStage::LaunchWritten,
        BootStage::ProviderReady,
    ] {
        fixture.service.emit_boot_stage(&tab_id, stage, None);
    }
    await_publication(&fixture, observation);
    assert_eq!(
        published_stage(&fixture.service, &tab_id),
        Some(serde_json::to_value(BootStage::ProviderReady).unwrap())
    );
}

#[test]
fn only_non_provisional_launch_milestones_publish() {
    for stage in [
        BootStage::ShellReady,
        BootStage::AgentLaunching,
        BootStage::LaunchWritten,
        BootStage::ProviderReady,
        BootStage::AgentReady,
        BootStage::SpawnFailed,
        BootStage::PtyReattached,
    ] {
        assert!(stage_publishes_mux_snapshot(stage), "{stage:?}");
    }
    for stage in [
        BootStage::PtyStarting,
        BootStage::PtySpawned,
        BootStage::ReaderReady,
        BootStage::LaunchDeadlineElapsed,
        BootStage::LaunchBindingUnconfirmed,
    ] {
        assert!(!stage_publishes_mux_snapshot(stage), "{stage:?}");
    }
}
