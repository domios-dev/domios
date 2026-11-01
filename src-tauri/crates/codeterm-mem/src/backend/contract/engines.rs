use super::*;
use crate::backend::{engines::ShadowUnit, ExportPage, StoreRow, StoreTable, StoreValue};
use std::collections::BTreeMap;

pub fn engine_pending_contract(store: &dyn MemStore) {
    let ctx = WriteContext::system("engine-contract");
    store.mark_pending(&ctx, "pending-a", "offline").unwrap();
    store.mark_pending(&ctx, "pending-a", "retry").unwrap();
    assert_eq!(store.pending_ids().unwrap(), vec!["pending-a"]);
    store.clear_pending(&ctx, "pending-a").unwrap();
    assert!(store.pending_ids().unwrap().is_empty());
}

pub fn engine_auxiliary_round_trip(source: &dyn MemStore, target: &dyn MemStore) {
    let ctx = WriteContext::system("engine-migration");
    source
        .mark_pending(&ctx, "pending-migration", "unavailable")
        .unwrap();
    let row = |values: Vec<(&str, StoreValue)>| StoreRow {
        values: values.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
        blobs: BTreeMap::new(),
    };
    let text = |s: &str| StoreValue::Text(s.to_owned());
    for (table, values) in [
        (
            StoreTable::LessonAudit,
            vec![
                ("id", StoreValue::Integer(7)),
                ("lesson_uid", text("lesson-uid")),
                ("machine_id", text("machine")),
                ("action", text("promoted")),
                ("ts", text("2026-01-01T00:00:00Z")),
                ("reason", text("verified")),
            ],
        ),
        (
            StoreTable::GraphPurgeLog,
            vec![
                ("id", StoreValue::Integer(8)),
                ("purged_at", text("2026-01-01T00:00:00Z")),
                ("count", StoreValue::Integer(2)),
                ("scope", StoreValue::Null),
                ("before", StoreValue::Null),
                ("reason_filter", StoreValue::Null),
                ("actor", text("contract")),
                ("details_json", text("{\"preserved\":true}")),
            ],
        ),
    ] {
        source
            .import_page(
                &ctx,
                &ExportPage {
                    table,
                    rows: vec![row(values)],
                    next: None,
                },
            )
            .unwrap();
    }
    let pages = super::export_all(source);
    for page in &pages {
        target.import_page(&ctx, page).unwrap();
    }
    for table in [
        StoreTable::VectorPending,
        StoreTable::LessonAudit,
        StoreTable::GraphPurgeLog,
    ] {
        assert_eq!(target.table_counts().unwrap()[&table], 1);
    }
    assert_eq!(super::export_all(target), pages);
    assert_eq!(target.pending_ids().unwrap(), vec!["pending-migration"]);
}

pub fn engine_shadow_contract(
    store: &dyn MemStore,
    active: &crate::embedder::ActiveEmbedder,
    target: &crate::embedder::ActiveEmbedder,
) {
    let ctx = WriteContext::system("shadow-contract");
    let id = store.save(&ctx, &item("shadow item"), false).unwrap().id;
    store.init_vectors(&ctx, &active.identity()).unwrap();
    let identity = target.identity();
    let stamp = serde_json::to_string(&identity).unwrap();
    store
        .prepare_engine_switch(&ctx, &identity, 0, true)
        .unwrap();
    let (units, total) = store.shadow_stale(&stamp).unwrap();
    assert_eq!(total, 1);
    assert_eq!(units.len(), 1);
    let unit: &ShadowUnit = &units[0];
    let vectors = vec![0u8; unit.texts.len() * identity.dimension * 4];
    assert!(store
        .shadow_checkpoint(&ctx, &stamp, unit, Some(&vectors), false, &|| false)
        .is_err());
    assert_eq!(store.shadow_stale(&stamp).unwrap().0.len(), 1);
    store
        .shadow_checkpoint(&ctx, &stamp, unit, Some(&vectors), false, &|| true)
        .unwrap();
    assert!(store.shadow_stale(&stamp).unwrap().0.is_empty());
    assert!(store
        .publish_shadow(&ctx, target, active, "wrong-target-stamp", &|| true)
        .is_err());
    store
        .update(
            &ctx,
            &id,
            &crate::store::MemUpdate {
                body: Some("updated live text during shadow build".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(!store
        .publish_shadow(&ctx, target, active, &stamp, &|| true)
        .unwrap());
    let (changed, _) = store.shadow_stale(&stamp).unwrap();
    assert_eq!(changed.len(), 1);
    let unit = &changed[0];
    let vectors = vec![0u8; unit.texts.len() * identity.dimension * 4];
    store
        .shadow_checkpoint(&ctx, &stamp, unit, Some(&vectors), false, &|| true)
        .unwrap();

    assert!(store
        .publish_shadow(&ctx, target, active, &stamp, &|| false)
        .is_err());
    assert_ne!(store.vector_stamp().unwrap(), Some(identity.clone()));
    assert!(store
        .publish_shadow(&ctx, target, active, &stamp, &|| true)
        .unwrap());
    assert_eq!(store.vector_stamp().unwrap(), Some(identity.clone()));
    assert_eq!(active.identity(), identity);
    let stale = StampedVector {
        identity: EmbedderIdentity {
            provider: "old".into(),
            model: "old".into(),
            dimension: 3,
        },
        vector: vec![0.; 3],
    };
    assert!(store.insert_vector(&ctx, &id, 0, &stale).is_err());
}
