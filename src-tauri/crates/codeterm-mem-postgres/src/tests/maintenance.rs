use super::*;
use codeterm_mem::backend::{engines::EngineStore, StoreValue};

fn seed(source: &SqliteMemStore, target: &PostgresMemStore) {
    let ctx = WriteContext::system("maintenance-fixture");
    let staging_dir = tempfile::tempdir().unwrap();
    let staging = SqliteMemStore::open_in_memory(staging_dir.path()).unwrap();
    for (title, kind, importance) in [
        ("promote", codeterm_mem::MemKind::Learning, 4.0),
        ("expire", codeterm_mem::MemKind::Note, 3.0),
        ("calibration", codeterm_mem::MemKind::Fact, 2.0),
        ("stale", codeterm_mem::MemKind::Daily, 1.0),
    ] {
        let mut item = contract::item(title);
        item.kind = kind;
        item.importance = importance;
        staging.save(&ctx, &item, false).unwrap();
    }
    let now = chrono::Utc::now();
    let old = (now - chrono::Duration::days(500)).to_rfc3339();
    let recent = (now - chrono::Duration::hours(1)).to_rfc3339();
    for mut page in export_all(&staging) {
        for row in &mut page.rows {
            if page.table == StoreTable::Items {
                row.values
                    .insert("created_at".into(), StoreValue::Text(old.clone()));
                row.values
                    .insert("updated_at".into(), StoreValue::Text(old.clone()));
                let title = match row.values.get("title").unwrap() {
                    StoreValue::Text(s) => s.clone(),
                    _ => panic!("title"),
                };
                if title == "promote" {
                    row.values
                        .insert("access_count".into(), StoreValue::Integer(10));
                    row.values
                        .insert("accessed_at".into(), StoreValue::Text(recent.clone()));
                }
            }
            if page.table == StoreTable::History {
                row.values
                    .insert("changed_at".into(), StoreValue::Text(old.clone()));
            }
        }
        source.import_page(&ctx, &page).unwrap();
        MemStore::import_page(target, &ctx, &page).unwrap();
    }
}
fn outcomes(store: &dyn MemStore) -> Vec<(String, f64, String)> {
    let mut out = Vec::new();
    for page in export_all(store) {
        if page.table == StoreTable::Items {
            for row in page.rows {
                let text = |key: &str| match &row.values[key] {
                    StoreValue::Text(s) => s.clone(),
                    _ => panic!("text"),
                };
                let importance = match row.values["importance"] {
                    StoreValue::Real(v) => v,
                    _ => panic!("importance"),
                };
                out.push((text("id"), importance, text("kind")));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}
#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn light_deep_normalize_match_sqlite_outcomes() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let sqlite = SqliteMemStore::open_in_memory(dir.path()).unwrap();
    seed(&sqlite, fixture.store());
    let ctx = WriteContext::system("dreaming-contract");
    for phase in ["dreaming-light", "dreaming-deep", "dreaming-normalize"] {
        let expected = sqlite
            .dream_worker(&ctx, phase, "fixture", Duration::from_secs(5))
            .unwrap();
        let actual = fixture
            .store()
            .dream_worker(&ctx, phase, "fixture", Duration::from_secs(5))
            .unwrap();
        assert_eq!(actual, expected, "{phase}");
        assert_eq!(outcomes(fixture.store()), outcomes(&sqlite), "{phase}");
    }
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn normalize_canonicalizes_conflicts_and_revives_only_present_endpoints() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let sqlite = SqliteMemStore::open_in_memory(dir.path()).unwrap();
    let ctx = WriteContext::system("normalize-contract");
    let a = sqlite
        .save(&ctx, &contract::item("graph-a"), false)
        .unwrap()
        .id;
    let b = sqlite
        .save(&ctx, &contract::item("graph-b"), false)
        .unwrap()
        .id;
    for (relation_type, to_id, valid_to) in [
        ("references", b.clone(), None),
        ("mentions", b.clone(), None),
        ("works_on", b.clone(), Some("2025-02-01T00:00:00Z".into())),
        ("broken", "absent".into(), None),
    ] {
        sqlite
            .graph_relate(
                &ctx,
                &codeterm_mem::Relation {
                    from_id: a.clone(),
                    to_id,
                    relation_type: relation_type.into(),
                    scope: "global".into(),
                    valid_from: "2025-01-01T00:00:00Z".into(),
                    valid_to,
                    confidence: 1.0,
                    properties: serde_json::json!({}),
                    reason: None,
                },
            )
            .unwrap();
    }
    for page in export_all(&sqlite) {
        MemStore::import_page(fixture.store(), &ctx, &page).unwrap();
    }
    let expected = sqlite.dream_normalize(&ctx).unwrap();
    let actual = fixture.store().dream_normalize(&ctx).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(actual["relations_canonicalized"], 1);
    assert_eq!(actual["broken_relations_invalidated"], 1);
    assert_eq!(actual["broken_relations_revived"], 1);
    let rows = fixture.store().graph_related(&a, None, None, true).unwrap();
    assert_eq!(rows.iter().filter(|r| r.valid_to.is_none()).count(), 2);
}
#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn calibration_and_stale_match_sqlite_and_nightly_cap() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let sqlite = SqliteMemStore::open_in_memory(dir.path()).unwrap();
    seed(&sqlite, fixture.store());
    let ctx = WriteContext::system("calibration-contract");
    assert_eq!(
        fixture.store().calibration_sweep(&ctx, true).unwrap(),
        sqlite.calibration_sweep(&ctx, true).unwrap()
    );
    assert_eq!(outcomes(fixture.store()), outcomes(&sqlite));
    assert_eq!(
        fixture.store().calibration_log("", 100, true).unwrap(),
        sqlite.calibration_log("", 100, true).unwrap()
    );
    let hygiene = codeterm_types::config::DreamHygieneConfig {
        stale_archive: true,
        stale_archive_cap: 1,
        ..Default::default()
    };
    let now = chrono::Utc::now();
    let pg = fixture
        .store()
        .nightly_calibration(&ctx, &hygiene, now)
        .unwrap();
    let sq = sqlite.nightly_calibration(&ctx, &hygiene, now).unwrap();
    assert_eq!(pg, sq);
    assert_eq!(outcomes(fixture.store()), outcomes(&sqlite));
    let pg = fixture
        .store()
        .nightly_calibration(&ctx, &hygiene, now)
        .unwrap();
    let sq = sqlite.nightly_calibration(&ctx, &hygiene, now).unwrap();
    assert_eq!(pg, sq);
}
