use super::*;
use crate::service::neighbors::{history::Entry, storage::Settings};
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};

struct Dropped(Option<tokio::sync::oneshot::Sender<()>>);
impl Drop for Dropped {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

fn target(tab: Option<String>) -> (Handle, tokio::sync::oneshot::Receiver<()>) {
    let (sender, dropped) = tokio::sync::oneshot::channel();
    let guard = Dropped(Some(sender));
    let task = tokio::spawn(async move {
        let _guard = guard;
        std::future::pending::<()>().await;
    });
    let (shutdown, _) = tokio::sync::watch::channel(false);
    (
        Handle {
            gateway_tab: tab,
            task,
            retry: Arc::new(tokio::sync::Notify::new()),
            shutdown,
        },
        dropped,
    )
}

#[tokio::test]
async fn removed_friendship_closes_concierge_retains_history_and_cancels_only_its_targets() {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let gateway = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&gateway).unwrap().agent_type = Some("fixture".into());
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friendship = "a".repeat(64);
    let other = "b".repeat(64);
    let mut settings = Settings::default();
    settings
        .bindings
        .insert(friendship.clone(), gateway.clone());
    metadata.save_settings(settings).await.unwrap();
    let entry: Entry = serde_json::from_value(json!({"direction":"out","author":"me","sender_uid":"owner","body":"retained text","at":1,"message_id":"c".repeat(32),"kind":"question","delivery_status":"delivered"})).unwrap();
    let generation = service.neighbor_history_generation();
    service
        .append_neighbor_history(&metadata, &friendship, entry, generation)
        .await
        .unwrap();
    let (removed, dropped) = target(Some(gateway.clone()));
    let (remaining, other_dropped) = target(None);
    {
        let mut listeners = service.neighbor_cell().listeners.lock();
        listeners.active.insert(friendship.clone(), removed);
        listeners.active.insert(other.clone(), remaining);
    }
    service
        .neighbor_cell()
        .pending_counts
        .lock()
        .insert(friendship.clone(), 3);
    service
        .end_missing_neighbor_friendships(
            &metadata,
            &json!({"friends":[{"friendship_id":other}]}),
            generation,
        )
        .await
        .unwrap();
    dropped.await.unwrap();
    assert!(!service
        .neighbor_cell()
        .listeners
        .lock()
        .active
        .contains_key(&friendship));
    assert!(service
        .neighbor_cell()
        .listeners
        .lock()
        .active
        .contains_key(&other));
    assert!(!service
        .neighbor_cell()
        .pending_counts
        .lock()
        .contains_key(&friendship));
    assert!(service.mux.read().get_tab(&gateway).is_none());
    assert!(service.mux.read().get_tab(&general).is_some());
    assert!(service
        .mux
        .read()
        .list_recently_closed(&workspace)
        .iter()
        .any(|tab| tab.tab_id == gateway));
    assert!(!metadata
        .settings()
        .await
        .unwrap()
        .bindings
        .contains_key(&friendship));
    let page = metadata.history_page(&friendship, None, 50).await.unwrap();
    assert_eq!(page["ended"], true);
    assert_eq!(page["entries"][0]["body"], "retained text");
    service
        .end_neighbor_friendship(&metadata, &friendship, generation)
        .await
        .unwrap();
    for task in service.stop_neighbor_friendship_listeners(&other) {
        task.abort();
    }
    other_dropped.await.unwrap();
}
