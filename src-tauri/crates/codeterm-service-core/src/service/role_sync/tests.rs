use super::*;
use crate::{
    multiplexer::state::MultiplexerState,
    ports::daemon_question::{DaemonQuestionHandler, DaemonQuestions},
    service::test_fixture::ServiceFixture,
};
use std::sync::Arc;

struct OtherQuestions;
impl DaemonQuestionHandler for OtherQuestions {
    fn answer(&self, _: &str, _: &str) -> Option<Result<Value, String>> {
        None
    }
}

fn fixture() -> ServiceFixture {
    let (mut mux, tab) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&tab).unwrap();
    let mut team = codeterm_types::team::Team::new(workspace, 0);
    team.id = "team_sync".into();
    mux.upsert_team(team);
    let fixture = ServiceFixture::new(mux);
    assert!(fixture
        .service
        .set_ext(TeamRoleStore::new(fixture.temp.path().join("roles")))
        .is_ok());
    assert!(fixture
        .service
        .register_host_extension(Arc::new(DaemonQuestions::new(Arc::new(OtherQuestions))))
        .is_ok());
    let mut registration = source("team_sync", "curator").unwrap();
    registration.instructions = "opaque-pinned-before".into();
    fixture
        .service
        .role_sync_store()
        .register(registration)
        .unwrap();
    fixture
}

#[test]
fn role_sync_requires_the_owner_choice_and_replay_is_a_noop() {
    let fixture = fixture();
    let service = &fixture.service;
    let preview = service.role_sync_preview("team_sync", "curator").unwrap();
    let response = service
        .request_role_sync(
            "team_sync",
            "curator",
            &preview.current_hash,
            &preview.replacement_hash,
        )
        .unwrap();
    let id = response["notification_id"].as_str().unwrap();
    assert_eq!(response["state"], "awaiting_owner");
    assert!(service.answer_notification(id, "Sync").is_err());
    assert!(service
        .answer_notification_as_owner(id, "unoffered")
        .is_err());
    assert_eq!(
        service.role_sync_store().registrations("team_sync")[0].instructions,
        "opaque-pinned-before"
    );
    assert_eq!(
        service.answer_notification_as_owner(id, "Sync").unwrap()["state"],
        "synced"
    );
    let synced = service.role_sync_store().registrations("team_sync");
    assert_eq!(
        synced[0].instructions,
        source("team_sync", "curator").unwrap().instructions
    );
    assert_eq!(
        service.answer_notification_as_owner(id, "Sync").unwrap()["delivered"],
        true
    );
    assert_eq!(
        service
            .request_role_sync(
                "team_sync",
                "curator",
                &preview.current_hash,
                &preview.replacement_hash
            )
            .unwrap()["state"],
        "unchanged"
    );
    assert_eq!(service.role_sync_store().registrations("team_sync"), synced);
}

#[test]
fn role_sync_rechecks_hashes_at_the_owner_tap_and_keeps_failure_unanswered() {
    let fixture = fixture();
    let service = &fixture.service;
    let preview = service.role_sync_preview("team_sync", "curator").unwrap();
    assert!(service
        .request_role_sync("team_sync", "curator", "stale", &preview.replacement_hash)
        .is_err());
    assert!(service
        .request_role_sync("team_sync", "curator", &preview.current_hash, "stale")
        .is_err());
    let response = service
        .request_role_sync(
            "team_sync",
            "curator",
            &preview.current_hash,
            &preview.replacement_hash,
        )
        .unwrap();
    let id = response["notification_id"].as_str().unwrap();
    let store = service.role_sync_store();
    let mut registrations = store.registrations("team_sync");
    registrations[0].instructions = "opaque-newer-pinned".into();
    crate::persisted_state::StateFile::new(store.path("team_sync"))
        .save(&serde_json::to_string(&registrations).unwrap())
        .unwrap();
    store.reload(Some("team_sync"));
    assert!(service.answer_notification_as_owner(id, "Sync").is_err());
    assert!(service
        .notif
        .lock()
        .answer_slot(id)
        .unwrap()
        .answered
        .is_none());
    assert_eq!(store.registrations("team_sync"), registrations);
    assert_eq!(
        service.answer_notification_as_owner(id, "Cancel").unwrap()["state"],
        "cancelled"
    );
}

#[test]
fn role_sync_preview_is_read_only_and_refuses_unregistered_or_custom_roles() {
    let fixture = fixture();
    let service = &fixture.service;
    let before = std::fs::read(service.role_sync_store().path("team_sync")).unwrap();
    let notifications = service.notif.lock().list(false).len();
    let preview = service.preview_role_sync("team_sync", "curator").unwrap();
    assert_eq!(preview["role"], "curator");
    assert_eq!(
        std::fs::read(service.role_sync_store().path("team_sync")).unwrap(),
        before
    );
    assert_eq!(service.notif.lock().list(false).len(), notifications);
    assert!(service.preview_role_sync("absent_team", "curator").is_err());
    assert!(service.preview_role_sync("team_sync", "worker").is_err());
    assert!(service.preview_role_sync("team_sync", "missing").is_err());
}
