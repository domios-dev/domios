use super::{close, started};
use crate::notification::push::NotifyLevel;
use crate::service::notification::{NotificationOrigin, PushIntent};
use crate::service::receiver_target::ReceiverTargetError;
use crate::service::ServiceContext;
use std::sync::Arc;

const RECEIVER_ID: &str = "round-trip-receiver";

struct SilentReceiver;

impl crate::plugin::WebhookReceiver for SilentReceiver {
    fn id(&self) -> &str {
        RECEIVER_ID
    }

    fn kind(&self) -> crate::plugin::WebhookReceiverKind {
        crate::plugin::WebhookReceiverKind::Pebble
    }

    fn receive(
        &self,
        _context: crate::plugin::WebhookReceiveContext,
    ) -> Result<Option<crate::plugin::WebhookDelivery>, crate::plugin::WebhookReceiveError> {
        Ok(None)
    }
}

fn register_receiver(service: &ServiceContext) {
    service
        .webhook_registry
        .register(RECEIVER_ID, Arc::new(SilentReceiver));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri service fixture requires Windows"
)]
fn delayed_send_create_update_delete_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let workspace_id = instance.fixture.service.mux.read().list_workspaces()[0]
        .id
        .clone();
    let tab_id = instance
        .fixture
        .service
        .create_named_tab(&workspace_id, "delayedSendTarget")
        .unwrap();
    let created = crate::delayed_send::create_message(
        &instance.fixture.service,
        tab_id,
        "initial delayed body".into(),
        Some(crate::delayed_send::now_ms() + 60_000),
        None,
        None,
    )
    .expect("create delayed send through ServiceContext");
    crate::delayed_send::update_message(
        &instance.fixture.service,
        created.message.id.clone(),
        Some("updated delayed body".into()),
        None,
        None,
        None,
    )
    .expect("update delayed send through ServiceContext");
    assert!(root.path().join("delayed_send.json").exists());

    let restarted = instance.restart(root.path());
    let restored = restarted
        .fixture
        .service
        .delayed_send
        .list(None)
        .into_iter()
        .find(|message| message.id == created.message.id)
        .expect("delayed send survives restart");
    assert_eq!(restored.text, "updated delayed body");
    crate::delayed_send::delete_message(&restarted.fixture.service, created.message.id)
        .expect("delete delayed send through ServiceContext");
    let restarted = restarted.restart(root.path());
    assert!(restarted.fixture.service.delayed_send.list(None).is_empty());
    close(restarted);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri service fixture requires Windows"
)]
fn sticky_notification_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let id = instance.fixture.service.create_notification_ext(
        "persistent notification".into(),
        Some("action-needed body".into()),
        Some("warning"),
        None,
        Some(true),
        NotificationOrigin::default(),
        PushIntent {
            level: Some(NotifyLevel::ActionNeeded),
            event_key: None,
        },
        None,
    );
    assert!(root.path().join("notifications.json").exists());

    let restarted = instance.restart(root.path());
    let notifications = restarted.fixture.service.notif.lock();
    let restored = notifications
        .notification(&id)
        .expect("unread sticky notification survives restart");
    assert_eq!(restored.title, "persistent notification");
    assert_eq!(restored.body.as_deref(), Some("action-needed body"));
    drop(notifications);
    close(restarted);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri service fixture requires Windows"
)]
fn receiver_target_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    register_receiver(&instance.fixture.service);
    let workspace_id = instance.fixture.service.mux.read().list_workspaces()[0]
        .id
        .clone();
    let tab_id = instance
        .fixture
        .service
        .create_named_tab(&workspace_id, "receiverTarget")
        .unwrap();
    instance
        .fixture
        .service
        .set_receiver_target(RECEIVER_ID, &tab_id)
        .expect("bind receiver through ServiceContext");

    let restarted = instance.restart(root.path());
    register_receiver(&restarted.fixture.service);
    let restored = restarted
        .fixture
        .service
        .receiver_target(RECEIVER_ID)
        .expect("receiver target is registered");
    assert_eq!(restored.tab_id.as_deref(), Some(tab_id.as_str()));
    assert_eq!(
        restarted
            .fixture
            .service
            .set_receiver_target("missing-receiver", &tab_id)
            .unwrap_err(),
        ReceiverTargetError::ReceiverNotFound
    );
    close(restarted);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri service fixture requires Windows"
)]
fn remote_worker_and_kpi_ledger_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let workspace_id = instance.fixture.service.mux.read().list_workspaces()[0]
        .id
        .clone();
    let tab_id = instance
        .fixture
        .service
        .create_named_tab(&workspace_id, "kpiWriter")
        .unwrap();
    instance.fixture.service.kpi.drain();
    instance.fixture.service.remote_workers.record_spawn(
        &tab_id,
        "peer-fixture",
        "remote-tab-fixture",
        "remote worker",
        Some("model-fixture".into()),
        crate::delayed_send::now_ms(),
    );
    instance.fixture.service.remote_workers.record_report(
        &tab_id,
        "peer-fixture",
        "remote-tab-fixture",
        "remote worker",
        Some("model-fixture".into()),
        "STATUS: working round-trip report",
        crate::delayed_send::now_ms(),
    );

    let restarted = instance.restart(root.path());
    let worker = restarted
        .fixture
        .service
        .remote_workers
        .for_orchestrator(&tab_id)
        .into_iter()
        .find(|worker| worker.remote_tab == "remote-tab-fixture")
        .expect("remote worker survives restart");
    assert_eq!(worker.last_message, "STATUS: working round-trip report");
    let rows = crate::agent::kpi_ledger::KpiLedger::open(root.path())
        .unwrap()
        .list_all()
        .unwrap();
    assert!(rows.iter().any(|row| {
        row.tab_id == tab_id && row.kind == crate::agent::kpi_ledger::KpiEventKind::TabCreated
    }));
    close(restarted);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri service fixture requires Windows"
)]
fn memory_fact_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let id = instance
        .fixture
        .service
        .mem
        .save(crate::mem::NewMemItem {
            kind: crate::mem::MemKind::Fact,
            scope: crate::mem::MemScope::Global,
            title: "persistence round-trip fact".into(),
            summary: Some("service memory mutation".into()),
            area: Some("test".into()),
            tags: vec!["round-trip".into()],
            body: "runtime memory record survives a service restart".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        })
        .expect("save memory through the ServiceContext-owned MemManager");
    assert!(root.path().join("mem").join("knowledge.db").exists());

    let restarted = instance.restart(root.path());
    let restored = restarted
        .fixture
        .service
        .mem
        .get(&id)
        .expect("read memory after service restart")
        .expect("memory record survives restart");
    assert_eq!(
        restored.body,
        "runtime memory record survives a service restart"
    );
    close(restarted);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri service fixture requires Windows"
)]
fn plugin_channel_registry_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let source = root.path().join("channel-source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("channel.json"),
        serde_json::json!({"schemaVersion": 1, "name": "fixture-channel", "plugins": []})
            .to_string(),
    )
    .unwrap();
    let context = |data_root| codeterm_plugins::PluginCtx {
        data_root,
        version: Some(env!("CARGO_PKG_VERSION").into()),
    };
    codeterm_plugins::run(
        context(root.path().to_path_buf()),
        codeterm_plugins::PluginAction::Channel(codeterm_plugins::PluginChannelAction::Add {
            url: format!("file://{}", source.display()),
            name: Some("fixture-channel".into()),
        }),
    )
    .expect("register plugin channel through the production channel command implementation");
    let restarted = instance.restart(root.path());
    let listing = codeterm_plugins::run(
        context(root.path().to_path_buf()),
        codeterm_plugins::PluginAction::Channel(codeterm_plugins::PluginChannelAction::List),
    )
    .expect("list channels after service restart");
    assert!(listing["channels"]
        .as_array()
        .unwrap()
        .iter()
        .any(|channel| { channel["name"] == "fixture-channel" }));
    close(restarted);
}
