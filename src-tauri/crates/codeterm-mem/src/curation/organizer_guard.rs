use super::merge::{run_merge, MergeRequest};
use crate::{
    store::{self, *},
    MemManager,
};

fn item(title: &str, kind: MemKind, area: &str) -> NewMemItem {
    NewMemItem {
        kind,
        scope: MemScope::Global,
        title: title.into(),
        summary: None,
        area: Some(area.into()),
        tags: vec![],
        body: format!("{title} content"),
        source: None,
        importance: 5.0,
        source_path: None,
        pinned: false,
    }
}

fn neighbor() -> MemAuthor {
    MemAuthor::neighbor("friend".into(), "friendship".into())
}

#[test]
fn pin_rekind_and_learning_promotion_refuse_neighbor_content() {
    let mem = MemManager::open_in_memory().unwrap();
    let id = with_author(neighbor(), || {
        mem.save(item("Neighbor", MemKind::Learning, "a"))
    })
    .unwrap();
    assert!(mem.promote_learning(&id).is_err());
    assert!(mem
        .update(
            &id,
            MemUpdate {
                pinned: Some(true),
                ..Default::default()
            }
        )
        .is_err());
    mem.with_conn(|conn| {
        for kind in ["rule", "preference", "skill"] {
            assert!(conn
                .execute(
                    "UPDATE mem_items SET kind=?1 WHERE id=?2",
                    rusqlite::params![kind, id]
                )
                .is_err());
        }
    });
}

#[test]
fn item_and_area_merges_refuse_instruction_targets_and_keep_neighbor_author() {
    for (kind, pinned) in [
        (MemKind::Rule, false),
        (MemKind::Preference, false),
        (MemKind::Skill, false),
        (MemKind::Fact, true),
        (MemKind::Fact, false),
    ] {
        let mem = MemManager::open_in_memory().unwrap();
        let lesser = with_author(neighbor(), || {
            mem.save(item("Neighbor", MemKind::Fact, "a"))
        })
        .unwrap();
        let mut target = item("Keeper", kind, "b");
        target.pinned = pinned;
        let keeper = with_author(MemAuthor::owner(None), || mem.save(target)).unwrap();
        let protected =
            pinned || matches!(kind, MemKind::Rule | MemKind::Preference | MemKind::Skill);
        mem.with_conn(|conn| {
            let moved = store::area_merge::merge_area(conn, mem.blobs_dir(), "a", "b", false, None);
            assert_eq!(moved.is_err(), protected);
            assert_eq!(item_author(conn, &lesser).unwrap().unwrap(), neighbor());
            let merged = run_merge(
                conn,
                mem.blobs_dir(),
                &MergeRequest {
                    keeper: &keeper,
                    lesser: &lesser,
                    body: Some("combined"),
                    conflict: false,
                },
                "organizer",
                3,
                chrono::Utc::now(),
            );
            assert_eq!(merged.is_err(), protected);
        });
        if !protected {
            assert_eq!(mem.get(&keeper).unwrap().unwrap().author, neighbor());
            assert_eq!(mem.get(&keeper).unwrap().unwrap().body, "combined");
        } else {
            assert_eq!(mem.get(&lesser).unwrap().unwrap().kind, "fact");
        }
    }
}

#[test]
fn neighbor_is_searchable_but_excluded_from_both_wakeup_passes() {
    let mem = MemManager::open_in_memory().unwrap();
    let id = with_author(neighbor(), || {
        mem.save(item("Neighbor marker", MemKind::Fact, "a"))
    })
    .unwrap();
    mem.with_conn(|conn| {
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM mem_fts WHERE mem_fts MATCH 'marker'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        for pinned in [0, 1] {
            conn.execute_batch("DROP TRIGGER IF EXISTS mem_neighbor_update_guard")
                .unwrap();
            conn.execute(
                "UPDATE mem_items SET is_pinned_wakeup=?1 WHERE id=?2",
                rusqlite::params![pinned, id],
            )
            .unwrap();
            for query in [
                crate::layers::score::PASS1_SQL,
                crate::layers::score::PASS2_SQL,
            ] {
                let ids: Vec<String> = conn
                    .prepare(query)
                    .unwrap()
                    .query_map([Option::<String>::None], |r| r.get(0))
                    .unwrap()
                    .collect::<Result<_, _>>()
                    .unwrap();
                assert!(!ids.contains(&id));
            }
        }
    });
    assert_eq!(
        serde_json::to_value(mem.get(&id).unwrap().unwrap()).unwrap()["author"]["account_uid"],
        "friend"
    );
}

#[test]
fn neighbor_title_summary_and_tags_cannot_keep_an_owner_signature() {
    for patch in [
        MemUpdate {
            title: Some("Neighbor title".into()),
            ..Default::default()
        },
        MemUpdate {
            summary: Some("Neighbor summary".into()),
            ..Default::default()
        },
        MemUpdate {
            tags: Some(vec!["neighbor-tag".into()]),
            ..Default::default()
        },
    ] {
        let mem = MemManager::open_in_memory().unwrap();
        let id = with_author(MemAuthor::owner(None), || {
            mem.save(item("Owner fact", MemKind::Fact, "a"))
        })
        .unwrap();
        with_author(neighbor(), || mem.update(&id, patch)).unwrap();
        assert_eq!(mem.get(&id).unwrap().unwrap().author, neighbor());
    }
}
