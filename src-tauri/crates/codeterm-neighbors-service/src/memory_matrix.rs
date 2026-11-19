use crate::prelude::*;
use crate::{
    mem::store::{with_author, MemAuthor, MemKind, MemScope, NewMemItem},
    service::test_fixture::ServiceFixture,
};
use codeterm_neighbors::permissions::Permission;
use serde_json::{json, Value};

pub fn grant() -> Permission {
    Permission {
        allowed: true,
        scope: "opaque".into(),
        memory_write_areas: vec!["project".into()],
        ..Default::default()
    }
}

pub async fn run<F, Fut>(case: &str, fixture: &ServiceFixture, caller: &str, call: F)
where
    F: Fn(String, Value) -> Fut,
    Fut: std::future::Future<Output = Result<Value, (u16, String)>>,
{
    fixture
        .seed_neighbor_concierge(caller, &"a".repeat(64), grant())
        .await;
    fixture
        .service
        .ext::<crate::config::ConfigState>()
        .lock()
        .memory
        .auto_extract = true;
    fixture
        .service
        .ext::<crate::config::ConfigState>()
        .lock()
        .api
        .token = Some("owner-secret".into());
    let authority = fixture
        .service
        .mux
        .read()
        .get_tab(caller)
        .unwrap()
        .neighbor_authority
        .clone()
        .unwrap();
    let mut body =
        json!({"kind":"fact","area":"project","title":"opaque title","body":"opaque memory body"});
    let mut method = "mem.neighbor_save";
    let mut expected = None;
    let mut previous = None;
    if case.starts_with("full_") {
        let mut item = crate::mem_ops::neighbor_save::validate(
            Some(&authority),
            &grant(),
            serde_json::from_value(body.clone()).unwrap(),
        )
        .unwrap();
        item.title = "taxonomy seed".into();
        fixture.service.mem.insert_raw(&item).unwrap();
        let mut permission = grant();
        permission.machine_access = codeterm_neighbors::permissions::MachineAccess::Full;
        permission.memory_write_areas.clear();
        fixture
            .seed_neighbor_concierge(caller, &authority.friendship_id, permission)
            .await;
    }
    match case {
        "full_fact" | "full_note" | "full_skill" => {
            body["kind"] = json!(case.strip_prefix("full_").unwrap());
        }
        "full_skill_replace" | "full_skill_pinned" => {
            body["kind"] = json!("skill");
            let mut permission = grant();
            permission.machine_access = codeterm_neighbors::permissions::MachineAccess::Full;
            let mut item = crate::mem_ops::neighbor_save::validate(
                Some(&authority),
                &permission,
                serde_json::from_value(body.clone()).unwrap(),
            )
            .unwrap();
            item.body = "previous skill body".into();
            item.pinned = case == "full_skill_pinned";
            let id = with_author(MemAuthor::owner(Some("original-owner".into())), || {
                fixture.service.mem.save(item)
            })
            .unwrap();
            previous = Some(fixture.service.mem.get(&id).unwrap().unwrap());
            if case == "full_skill_pinned" {
                expected = Some("neighbor_memory_title_conflict");
            }
        }
        "full_unknown_area" | "full_unassigned_area" => {
            body["area"] = json!(if case == "full_unknown_area" {
                "unknown"
            } else {
                "(none)"
            });
            expected = Some("neighbor_memory_area_not_granted");
        }
        "full_secret" => {
            body["kind"] = json!("secret");
            expected = Some("neighbor_memory_kind_not_allowed");
        }
        "full_skill_credentials" => {
            body["kind"] = json!("skill");
            body["body"] = json!("ghp_0123456789abcdefghijklmnopqrstuvwxyz");
            expected = Some("neighbor_memory_contains_credentials");
        }
        "full_narrowed" | "full_revoked" | "full_paused" => {
            let saved = call(method.into(), body.clone()).await.unwrap();
            previous = Some(
                fixture
                    .service
                    .mem
                    .get(saved["id"].as_str().unwrap())
                    .unwrap()
                    .unwrap(),
            );
            if case == "full_paused" {
                fixture.pause_neighbor_fixture(caller).await;
                expected = Some("neighbor_paused");
            } else {
                let mut permission = grant();
                permission.machine_access = codeterm_neighbors::permissions::MachineAccess::Full;
                if case == "full_revoked" {
                    permission.allowed = false;
                    expected = Some("neighbor_memory_write_not_allowed");
                } else {
                    permission.memory_write_areas = vec!["private".into()];
                    expected = Some("neighbor_memory_area_not_granted");
                }
                fixture
                    .seed_neighbor_concierge(caller, &authority.friendship_id, permission)
                    .await;
            }
        }
        "folder_empty_write_list" => {
            let mut permission = grant();
            permission.machine_access = codeterm_neighbors::permissions::MachineAccess::Folder;
            permission.folder = Some(std::env::temp_dir().to_string_lossy().into_owned());
            permission.memory_write_areas.clear();
            fixture
                .seed_neighbor_concierge(caller, &authority.friendship_id, permission)
                .await;
            expected = Some("neighbor_memory_write_not_allowed");
        }
        "area_b_refused" => {
            body["area"] = json!("private");
            expected = Some("neighbor_memory_area_not_granted");
        }
        "empty_write_list" => {
            let mut permission = grant();
            permission.memory_write_areas.clear();
            fixture
                .seed_neighbor_concierge(caller, &authority.friendship_id, permission)
                .await;
            expected = Some("neighbor_memory_write_not_allowed");
        }
        "mem_save_denied" => {
            method = "mem.save";
            expected = Some("neighbor_capability_denied");
        }
        "rule_refused" | "preference_refused" | "skill_refused" | "script_refused" => {
            body["kind"] = json!(case.strip_suffix("_refused").unwrap());
            expected = Some("neighbor_memory_kind_not_allowed");
        }
        "credentials_title" | "credentials_body" | "credentials_summary" | "credentials_tags" => {
            let field = case.strip_prefix("credentials_").unwrap();
            body[field] = if field == "tags" {
                json!(["ghp_0123456789abcdefghijklmnopqrstuvwxyz"])
            } else {
                json!("ghp_0123456789abcdefghijklmnopqrstuvwxyz")
            };
            expected = Some("neighbor_memory_contains_credentials");
        }
        "over_cap_body" => {
            body["body"] = json!("é".repeat(8193));
            expected = Some("neighbor_memory_too_large");
        }
        "over_cap_title" => {
            body["title"] = json!("é".repeat(101));
            expected = Some("neighbor_memory_too_large");
        }
        "over_cap_total" => {
            body["summary"] = json!("x".repeat(32752));
            expected = Some("neighbor_memory_too_large");
        }
        "forced_fields" => {
            body["pinned"] = json!(true);
            body["scope"] = json!("workspace:foreign");
            body["importance"] = json!(99);
        }
        "forged_author_ignored" => {
            body["author"] = json!({"kind":"owner","account_uid":"forged"});
            body["uid"] = json!("forged");
            body["author_uid"] = json!("forged");
        }
        "owner_refused" => {
            method = "owner";
            expected = Some("neighbor_memory_write_not_allowed");
        }
        "missing_friend_author" => {
            fixture
                .service
                .try_ext::<codeterm_service_core::mem_ops::author::FixtureNeighborAuthors>()
                .unwrap()
                .0
                .write()
                .clear();
            expected = Some("neighbor_memory_author_unavailable");
        }
        "revoked_area" | "paused_friend" => {
            let saved = call(method.into(), body.clone()).await.unwrap();
            previous = Some(
                fixture
                    .service
                    .mem
                    .get(saved["id"].as_str().unwrap())
                    .unwrap()
                    .unwrap(),
            );
            if case == "revoked_area" {
                let mut permission = grant();
                permission.memory_write_areas = vec!["private".into()];
                fixture
                    .seed_neighbor_concierge(caller, &authority.friendship_id, permission)
                    .await;
                expected = Some("neighbor_memory_area_not_granted");
            } else {
                fixture.pause_neighbor_fixture(caller).await;
                expected = Some("neighbor_paused");
            }
        }
        "replace_inside_grant" | "replace_outside_grant" | "replace_pinned" => {
            let item = NewMemItem {
                kind: MemKind::Fact,
                scope: MemScope::Global,
                title: "opaque title".into(),
                body: "original owner body".into(),
                summary: None,
                area: Some(
                    if case == "replace_outside_grant" {
                        "private"
                    } else {
                        "project"
                    }
                    .into(),
                ),
                tags: vec![],
                source: None,
                source_path: None,
                importance: 3.0,
                pinned: case == "replace_pinned",
            };
            let id = with_author(MemAuthor::owner(Some("original-owner".into())), || {
                fixture.service.mem.save(item)
            })
            .unwrap();
            previous = Some(fixture.service.mem.get(&id).unwrap().unwrap());
            if case != "replace_inside_grant" {
                expected = Some("neighbor_memory_title_conflict");
            }
        }
        "saved_in_granted_area" | "never_extracts" => {}
        _ => panic!("Unknown matrix case {case}"),
    }
    let workers_before = fixture.service.workers.list().len();
    let result = call(method.into(), body).await;
    if let Some(expected) = expected {
        let (status, message) = result.unwrap_err();
        assert_eq!(status, 403, "{case}: {message}");
        assert_eq!(
            message.split(':').next().unwrap(),
            expected,
            "{case}: {message}"
        );
        if let Some(previous) = previous {
            let stored = fixture.service.mem.get(&previous.id).unwrap().unwrap();
            assert_eq!(stored.body, previous.body);
            assert_eq!(stored.author, previous.author);
            let pinned = fixture.service.mem.list_pinned(100).unwrap();
            assert_eq!(
                pinned.iter().any(|item| item.id == stored.id),
                matches!(case, "replace_pinned" | "full_skill_pinned")
            );
        }
    } else {
        let saved = result.unwrap();
        let stored = fixture
            .service
            .mem
            .get(saved["id"].as_str().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(stored.area.as_deref(), Some("project"));
        assert_eq!(
            stored.kind,
            if case == "full_note" {
                "note"
            } else if matches!(case, "full_skill" | "full_skill_replace") {
                "skill"
            } else {
                "fact"
            }
        );
        assert_eq!(stored.body, "opaque memory body");
        assert_eq!(
            stored.author,
            MemAuthor::neighbor("friend-account-uid".into(), authority.friendship_id)
        );
        assert_ne!(
            stored.author.account_uid.as_deref(),
            Some(authority.uid.as_str())
        );
        assert!(!fixture
            .service
            .mem
            .list_pinned(100)
            .unwrap()
            .iter()
            .any(|item| item.id == stored.id));
        assert_eq!(stored.scope, "global");
        if case == "forced_fields" {
            assert_eq!(stored.importance, 5.0);
        }
        if let Some(previous) = previous {
            assert_eq!(stored.id, previous.id);
            let history = fixture.service.mem.get_item_history(&stored.id).unwrap();
            assert!(history.iter().any(|entry| entry.author == previous.author));
        }
    }
    assert_eq!(fixture.service.workers.list().len(), workers_before);
}
