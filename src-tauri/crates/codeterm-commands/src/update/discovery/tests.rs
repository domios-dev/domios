use super::*;

fn app_info(check_path: Option<&str>) -> codeterm_update::update_state::UpdateInfo {
    codeterm_update::update_state::UpdateInfo {
        availability: AvailabilityItem::release("app", UpdateKind::App, "1.0.0", "1.0.0", true),
        current_version: "1.0.0".into(),
        latest_version: "1.0.0".into(),
        update_available: false,
        install_method: codeterm_update::InstallMethod::Unknown,
        update_command: None,
        download_url: None,
        release_url: String::new(),
        release_notes: None,
        published_at: None,
        check_path: check_path.map(str::to_owned),
    }
}

#[test]
fn a_check_records_which_path_served_it() {
    let mut snapshot = AvailabilitySnapshot::default();
    apply_app_check(&mut snapshot, Ok((app_info(Some("github_api")), None)));
    assert_eq!(snapshot.served_by["app"], "github_api");
    assert!(snapshot.errors.is_empty());
    assert!(app_checked(&snapshot));
}

#[test]
fn both_failures_stay_in_the_snapshot_diagnostics_and_the_app_keeps_its_last_known_state() {
    let both = "direct download: HTTP 504; GitHub API fallback: GitHub API returned HTTP 403";
    let mut cached = AvailabilitySnapshot::default();
    apply_app_check(&mut cached, Ok((app_info(None), Some(both.into()))));
    assert_eq!(cached.errors["app"], both);
    assert_eq!(cached.items[0].reason, "cached_check");

    let mut unknown = AvailabilitySnapshot::default();
    apply_app_check(&mut unknown, Err(both.into()));
    assert_eq!(unknown.errors["app"], both);
    assert_eq!(unknown.items[0].reason, "not_checked");
    assert!(!app_checked(&unknown));
}

#[test]
fn a_check_deferred_by_its_interval_is_not_a_failure() {
    let mut snapshot = AvailabilitySnapshot::default();
    apply_app_check(
        &mut snapshot,
        Err(codeterm_update::checker::CHECK_DEFERRED.into()),
    );
    assert!(snapshot.errors.is_empty());
    assert_eq!(snapshot.items[0].reason, "not_checked");
}

#[test]
fn a_frontend_release_that_was_never_published_is_no_offer() {
    let local =
        || AvailabilityItem::release("frontend", UpdateKind::Frontend, "1.0.0", "1.0.0", true);
    let missing = frontend_offer(Err("release asset not found".into()), local).unwrap();
    assert_eq!(missing.state, codeterm_plugins::AvailabilityState::UpToDate);
    assert_eq!(
        frontend_offer(Err("server returned HTTP 504".into()), local),
        Err("server returned HTTP 504".to_string())
    );
}

#[test]
fn cached_checks_only_enable_an_app_update_when_a_newer_payload_exists() {
    let current = AvailabilityItem::release("app", UpdateKind::App, "1.0.0", "1.0.0", true);
    assert_eq!(cached_app_availability(current).reason, "cached_check");
    let incompatible = AvailabilityItem::release("app", UpdateKind::App, "1.0.0", "2.0.0", false);
    assert_eq!(cached_app_availability(incompatible).reason, "cached_check");
    let newer = AvailabilityItem::release("app", UpdateKind::App, "1.0.0", "2.0.0", true);
    assert_eq!(cached_app_availability(newer).reason, "cached_manifest");
}
#[test]
fn action_completion_replaces_installed_states_and_preserves_other_updates() {
    let mut snapshot = AvailabilitySnapshot::default();
    snapshot.items.push(AvailabilityItem::release(
        "app",
        UpdateKind::App,
        "1.0.0",
        "2.0.0",
        true,
    ));
    replace_plugins(
        &mut snapshot,
        vec![AvailabilityItem::release(
            "lark",
            UpdateKind::Plugin,
            "0.1.2",
            "0.1.3",
            true,
        )],
    );
    assert_eq!(
        snapshot.items[1].state,
        codeterm_plugins::AvailabilityState::UpdateAvailable
    );
    replace_plugins(
        &mut snapshot,
        vec![AvailabilityItem::release(
            "lark",
            UpdateKind::Plugin,
            "0.1.3",
            "0.1.3",
            true,
        )],
    );
    assert_eq!(
        snapshot.items[1].state,
        codeterm_plugins::AvailabilityState::UpToDate
    );
    assert_eq!(
        snapshot.items[0].state,
        codeterm_plugins::AvailabilityState::UpdateAvailable
    );
    replace_plugins(&mut snapshot, vec![]);
    assert_eq!(snapshot.items.len(), 1);
}

#[test]
fn offline_attempt_is_cached_and_keeps_the_installed_app_visible() {
    let mut snapshot = AvailabilitySnapshot::default();
    record_failure(&mut snapshot, "offline", 80_000);
    assert!(cache::fresh(snapshot.checked_at_ms, 100_000));
    assert_eq!(snapshot.errors["discovery"], "offline");
    assert_eq!(snapshot.items.len(), 1);
    assert_eq!(snapshot.items[0].kind, UpdateKind::App);
    assert_eq!(snapshot.items[0].latest, None);
    ensure_app(&mut snapshot);
    assert_eq!(snapshot.items.len(), 1);
}

#[tokio::test]
async fn fresh_background_result_updates_cache_and_emits_local_changes_once() {
    use codeterm_service_core::ports::host::UiEvents;
    #[derive(Default)]
    struct Events(parking_lot::Mutex<Vec<(String, serde_json::Value)>>);
    impl UiEvents for Events {
        fn emit(&self, event: &str, payload: serde_json::Value) {
            self.0.lock().push((event.into(), payload));
        }
        fn emit_to(&self, _: &str, event: &str, payload: serde_json::Value) {
            self.emit(event, payload);
        }
    }
    struct RestoreCache(Option<AvailabilitySnapshot>);
    impl Drop for RestoreCache {
        fn drop(&mut self) {
            *LAST.lock() = self.0.take();
        }
    }
    let _restore = RestoreCache(LAST.lock().take());
    let events = Arc::new(Events::default());
    let ui: Arc<dyn UiEvents> = events.clone();
    let mut previous = AvailabilitySnapshot::default();
    previous.checked_at_ms = Some(cache::now_ms());
    ensure_app(&mut previous);
    publish(&ui, &previous);
    events.0.lock().clear();
    let mut fresh = previous;
    let mut frontend =
        AvailabilityItem::release("frontend", UpdateKind::Frontend, "1.0.0", "1.1.0", true);
    frontend.state = codeterm_plugins::AvailabilityState::ReadyToApply;
    replace_frontend(&mut fresh, frontend);
    replace_plugins(
        &mut fresh,
        vec![AvailabilityItem::release(
            "local-plugin",
            UpdateKind::Plugin,
            "1.0.0",
            "1.1.0",
            true,
        )],
    );
    assert!(cache::fresh(fresh.checked_at_ms, cache::now_ms()));
    complete_background_check(&ui, async { Ok(fresh.clone()) }).await;
    assert_eq!(cached_snapshot(), fresh);
    assert_eq!(
        *events.0.lock(),
        vec![(
            "update-availability-changed".into(),
            serde_json::to_value(&fresh).unwrap(),
        )]
    );
    complete_background_check(&ui, async { Ok(fresh.clone()) }).await;
    assert_eq!(
        events.0.lock().len(),
        1,
        "equal network or cached results must not emit twice"
    );
}
