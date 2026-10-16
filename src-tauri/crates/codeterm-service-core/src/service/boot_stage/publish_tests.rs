use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use std::sync::atomic::Ordering;
use std::sync::Arc;

fn find_tab<'a>(value: &'a serde_json::Value, tab_id: &str) -> Option<&'a serde_json::Value> {
    match value {
        serde_json::Value::Object(map)
            if map.get("tab_id").and_then(|v| v.as_str()) == Some(tab_id) =>
        {
            Some(value)
        }
        serde_json::Value::Object(map) => map.values().find_map(|v| find_tab(v, tab_id)),
        serde_json::Value::Array(items) => items.iter().find_map(|v| find_tab(v, tab_id)),
        _ => None,
    }
}

fn find_key<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    match value {
        serde_json::Value::Object(map) => map
            .get(key)
            .or_else(|| map.values().find_map(|v| find_key(v, key))),
        serde_json::Value::Array(items) => items.iter().find_map(|v| find_key(v, key)),
        _ => None,
    }
}

fn published_stage(service: &ServiceContext, tab_id: &str) -> Option<serde_json::Value> {
    let snapshot: serde_json::Value =
        serde_json::from_slice(&service.published_mux_state()).ok()?;
    find_key(find_tab(&snapshot, tab_id)?, "boot_stage").cloned()
}

fn await_published_stage(fixture: &ServiceFixture, tab_id: &str, stage: BootStage) {
    let expected = serde_json::to_value(stage).unwrap();
    let mut events = fixture.service.events.subscribe();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        while published_stage(&fixture.service, tab_id).as_ref() != Some(&expected) {
            tokio::time::timeout_at(deadline, events.recv())
                .await
                .expect("published snapshot carries the committed boot stage")
                .ok();
        }
    });
}

#[test]
fn boot_stage_commit_reaches_published_snapshot_without_other_mutation() {
    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let before = fixture.service.published_mux_state();
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
    await_published_stage(&fixture, &tab_id, BootStage::LaunchWritten);
    assert_ne!(fixture.service.published_mux_state(), before);
}

#[test]
fn each_publishing_stage_requests_one_coalesced_snapshot() {
    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let stats = fixture.service.ext::<Arc<crate::service::MuxEmitStats>>();
    let requests = || stats.requests.load(Ordering::Relaxed);
    let silent = requests();
    fixture
        .service
        .emit_boot_stage(&tab_id, BootStage::PtySpawned, None);
    let baseline = requests() - silent;
    let burst = [
        BootStage::AgentLaunching,
        BootStage::LaunchWritten,
        BootStage::ProviderReady,
    ];
    for stage in burst {
        let start = requests();
        fixture.service.emit_boot_stage(&tab_id, stage, None);
        assert_eq!(requests() - start, baseline + 1, "{stage:?}");
    }
    await_published_stage(&fixture, &tab_id, BootStage::ProviderReady);
    assert!(stats.fires.load(Ordering::Relaxed) <= requests());
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
