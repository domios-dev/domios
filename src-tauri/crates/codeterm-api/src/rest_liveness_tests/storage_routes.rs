use super::*;
use axum::extract::{Path, Query, State};
use futures_util::FutureExt;

struct HeldDependency {
    release: Option<std::sync::mpsc::Sender<()>>,
    holder: Option<std::thread::JoinHandle<()>>,
}

impl HeldDependency {
    async fn new(
        hold: impl FnOnce(tokio::sync::oneshot::Sender<()>, std::sync::mpsc::Receiver<()>)
            + Send
            + 'static,
    ) -> Self {
        let (locked, ready) = tokio::sync::oneshot::channel();
        let (release, receive) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || hold(locked, receive));
        ready.await.unwrap();
        Self {
            release: Some(release),
            holder: Some(holder),
        }
    }
}

impl Drop for HeldDependency {
    fn drop(&mut self) {
        self.release.take().unwrap().send(()).ok();
        self.holder.take().unwrap().join().unwrap();
    }
}

async fn health_responds(state: crate::routes::AppState) {
    assert_eq!(tokio::spawn(async { 42 }).await.unwrap(), 42);
    let response = tokio::time::timeout(
        Duration::from_millis(250),
        crate::routes::create_router(state).oneshot(request("/api/health")),
    )
    .await
    .expect("health must answer while the dependency is held")
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 16 * 1024)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["status"], "ok");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn rest_health_answers_while_services_and_activity_wait_on_sql() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = app_state(&fixture);
    let connection = state.svc.services_store.conn();
    let held = HeldDependency::new(move |locked, release| {
        let _guard = connection.lock();
        locked.send(()).unwrap();
        release.recv_timeout(Duration::from_secs(3)).ok();
    })
    .await;
    let mut activity = Box::pin(crate::services_routes::list_activity(
        State(state.clone()),
        Default::default(),
        Query(crate::services_routes::ActivityQuery {
            source: None,
            since: None,
            limit: Some(usize::MAX),
        }),
    ));
    let mut services = Box::pin(crate::services_routes::list_services(
        State(state.clone()),
        Default::default(),
        Query(crate::services_routes::ListQuery {
            kind: None,
            enabled: None,
        }),
    ));
    state.config.lock().api.token = Some("inventory-fixture".into());
    let mut inventory_headers = axum::http::HeaderMap::new();
    inventory_headers.insert("authorization", "Bearer inventory-fixture".parse().unwrap());
    let mut inventory = Box::pin(crate::mesh_inventory::inventory(
        State(state.clone()),
        inventory_headers,
    ));
    assert!(activity.as_mut().now_or_never().is_none());
    assert!(services.as_mut().now_or_never().is_none());
    assert!(inventory.as_mut().now_or_never().is_none());
    health_responds(state.clone()).await;
    let cron = state.cron.clone();
    tokio::time::timeout(
        Duration::from_millis(250),
        tokio::task::spawn_blocking(move || cron.lock().list().len()),
    )
    .await
    .expect("cron must remain independent of service SQL")
    .unwrap();
    assert!(state.svc.services_store.conn().try_lock().is_none());
    drop(held);
    assert!(activity.await.unwrap().0.is_empty());
    assert!(services.await.unwrap().0.is_empty());
    assert!(inventory.await.is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn rest_health_answers_while_task_list_and_show_wait_on_plan_sql() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = app_state(&fixture);
    let plans = state.svc.plan_store.clone();
    let held = HeldDependency::new(move |locked, release| {
        plans.with_database_held_for_test(|| {
            locked.send(()).unwrap();
            release.recv_timeout(Duration::from_secs(3)).ok();
        });
    })
    .await;
    let mut list = Box::pin(crate::task_routes::list(
        State(state.clone()),
        Default::default(),
        Query(crate::task_routes::ListTasksQuery {
            project: None,
            active: None,
            all: None,
            mine: None,
            unassigned: None,
            plan: None,
            status: None,
        }),
    ));
    let mut show = Box::pin(crate::task_routes::show(
        State(state.clone()),
        Path("missing".into()),
    ));
    assert!(list.as_mut().now_or_never().is_none());
    assert!(show.as_mut().now_or_never().is_none());
    health_responds(state).await;
    assert!(list.as_mut().now_or_never().is_none());
    assert!(show.as_mut().now_or_never().is_none());
    drop(held);
    assert_eq!(list.await.unwrap().0["tasks"], serde_json::json!([]));
    assert_eq!(show.await.unwrap_err().0, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn rest_health_answers_while_theme_preparation_waits_on_config() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = app_state(&fixture);
    let config = state.config.clone();
    let held = HeldDependency::new(move |locked, release| {
        let _guard = config.lock();
        locked.send(()).unwrap();
        release.recv_timeout(Duration::from_secs(3)).ok();
    })
    .await;
    let mut apply = Box::pin(crate::theme_routes::apply_theme_with(
        state.clone(),
        serde_json::from_value(serde_json::json!({"name":"github-dark"})).unwrap(),
        |_, _| Err("fixture persistence refusal".into()),
    ));
    assert!(apply.as_mut().now_or_never().is_none());
    health_responds(state.clone()).await;
    assert!(state.config.try_lock().is_none());
    drop(held);
    assert_eq!(apply.await.unwrap_err().1, "fixture persistence refusal");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn rest_health_answers_while_notification_reads_and_writes_wait_on_store() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = app_state(&fixture);
    let notifications = state.svc.notif.clone();
    let held = HeldDependency::new(move |locked, release| {
        let _guard = notifications.lock();
        locked.send(()).unwrap();
        release.recv_timeout(Duration::from_secs(3)).ok();
    })
    .await;
    let mut list = Box::pin(crate::notification_routes::list_notifications(
        State(state.clone()),
        Default::default(),
        Query(serde_json::from_value(serde_json::json!({})).unwrap()),
    ));
    let mut mark = Box::pin(crate::notification_routes::mark_notification_read(
        State(state.clone()),
        Default::default(),
        Path("missing".into()),
    ));
    assert!(list.as_mut().now_or_never().is_none());
    assert!(mark.as_mut().now_or_never().is_none());
    health_responds(state.clone()).await;
    assert!(state.svc.notif.try_lock().is_none());
    drop(held);
    assert!(list.await.unwrap().0.as_array().unwrap().is_empty());
    assert_eq!(mark.await.unwrap_err().0, StatusCode::NOT_FOUND);
}
