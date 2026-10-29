use crate::backend::{MemStore, SaveError, WriteContext};
use crate::embedder::{EmbedderIdentity, StampedVector};
use crate::store::{MemAuthor, ReplaceGuard, ReplaceGuardError};
use crate::{MemKind, MemScope, NewMemItem, Relation, SearchQuery};
pub mod blueprint;
pub mod recall;
#[cfg(test)]
mod transfer;

pub fn item(title: &str) -> NewMemItem {
    NewMemItem {
        kind: MemKind::Fact,
        scope: MemScope::Global,
        title: title.into(),
        summary: None,
        area: Some("contract-source".into()),
        tags: vec!["contract".into()],
        body: "contract astrolabe observations".into(),
        source: None,
        importance: 2.0,
        source_path: None,
        pinned: false,
    }
}

pub fn save_replace_history(store: &dyn MemStore) {
    let owner = WriteContext::new(
        MemAuthor::owner(Some("contract-owner".into())),
        "contract-create",
    );
    let first = store
        .save(&owner, &item("contract history"), false)
        .unwrap();
    assert_eq!(first.outcome, "created");
    let unchanged = store
        .save(&owner, &item("contract history"), false)
        .unwrap();
    assert_eq!(unchanged.id, first.id);
    assert_eq!(unchanged.outcome, "unchanged");
    let neighbor = WriteContext::new(
        MemAuthor::neighbor("contract-friend".into(), "contract-friendship".into()),
        "contract-replace",
    );
    let mut replacement = item("contract history");
    replacement.body = "replacement contract body".into();
    let replaced = store.save(&neighbor, &replacement, true).unwrap();
    assert_eq!(replaced.id, first.id);
    assert_eq!(replaced.outcome, "updated");
    let got = store.get(&first.id).unwrap().unwrap();
    assert_eq!(got.body, "replacement contract body");
    assert_eq!(got.author, neighbor.author);
    let history = store.get_item_history(&first.id).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].author, owner.author);
    assert_eq!(history[0].changed_by, "contract-create");
    assert_eq!(history[1].author, owner.author);
    assert_eq!(history[1].changed_by, "contract-replace");
    assert_eq!(
        history[1].old_body.as_deref(),
        Some("contract astrolabe observations")
    );
    assert_eq!(
        history[1].new_body.as_deref(),
        Some("replacement contract body")
    );
    assert!(store.delete(&neighbor, &first.id).unwrap());
    assert!(store.get(&first.id).unwrap().is_none());
    let history = store.get_item_history(&first.id).unwrap();
    assert_eq!(history.last().unwrap().change_type, "delete");
    assert_eq!(history.last().unwrap().author, neighbor.author);
    assert_eq!(history.last().unwrap().changed_by, "contract-replace");
}

pub fn fts_hit(store: &dyn MemStore) {
    let ctx = WriteContext::system("contract");
    let id = store
        .save(&ctx, &item("contract lexical"), false)
        .unwrap()
        .id;
    let hits = store
        .search(
            &SearchQuery {
                query: "astrolabe".into(),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].item.id, id);
    assert_eq!(store.access_count(&id).unwrap(), Some(0));
    store
        .record_access(&ctx, &[id.clone(), id.clone()])
        .unwrap();
    assert_eq!(store.access_count(&id).unwrap(), Some(1));
}

pub fn vector_stamp_refusal(store: &dyn MemStore) {
    let ctx = WriteContext::system("contract");
    let id = store
        .save(&ctx, &item("contract vector"), false)
        .unwrap()
        .id;
    let identity = EmbedderIdentity {
        provider: "contract".into(),
        model: "one".into(),
        dimension: 3,
    };
    store.init_vectors(&ctx, &identity).unwrap();
    assert_eq!(store.vector_stamp().unwrap(), Some(identity.clone()));
    let vector = StampedVector {
        identity: identity.clone(),
        vector: vec![1.0, 0.0, 0.0],
    };
    assert_eq!(
        store.insert_vector(&ctx, &id, 0, &vector).unwrap(),
        crate::embedder::index::InsertOutcome::Inserted
    );
    assert_eq!(store.vector_search(&vector, 1).unwrap()[0].0, id);
    let wrong = StampedVector {
        identity: EmbedderIdentity {
            model: "two".into(),
            ..identity.clone()
        },
        vector: vector.vector,
    };
    assert!(store
        .insert_vector(&ctx, &id, 0, &wrong)
        .unwrap_err()
        .contains("mixed-model"));
    assert!(store
        .vector_search(&wrong, 1)
        .unwrap_err()
        .contains("mixed-model"));
    assert_eq!(store.vector_stamp().unwrap(), Some(identity));
}

pub fn graph_relate_unrelate(store: &dyn MemStore) {
    let ctx = WriteContext::system("contract");
    let a = store
        .save(&ctx, &item("contract graph a"), false)
        .unwrap()
        .id;
    let b = store
        .save(&ctx, &item("contract graph b"), false)
        .unwrap()
        .id;
    store
        .graph_relate(
            &ctx,
            &Relation {
                from_id: a.clone(),
                relation_type: "mentions".into(),
                to_id: b.clone(),
                scope: "global".into(),
                valid_from: "2026-01-01T00:00:00Z".into(),
                valid_to: None,
                confidence: 0.75,
                properties: serde_json::json!({"contract":true}),
                reason: Some("contract".into()),
            },
        )
        .unwrap();
    assert_eq!(
        store
            .graph_related(&a, Some("mentions"), None, false)
            .unwrap()
            .len(),
        1
    );
    assert!(store
        .graph_invalidate(&ctx, &a, "mentions", &b, Some("2026-02-01T00:00:00Z"))
        .unwrap());
    assert!(store
        .graph_related(&a, Some("mentions"), None, false)
        .unwrap()
        .is_empty());
    let history = store
        .graph_related(&a, Some("mentions"), None, true)
        .unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].valid_to.as_deref(), Some("2026-02-01T00:00:00Z"));
    store.graph_alias(&ctx, "contract-alias", &a).unwrap();
    assert_eq!(store.graph_resolve("contract-alias").unwrap(), a);
}

pub fn area_merge_attribution(store: &dyn MemStore) {
    let author = WriteContext::new(MemAuthor::owner(None), "contract-create");
    let id = store
        .save(&author, &item("contract area"), false)
        .unwrap()
        .id;
    let ctx = WriteContext::new(
        MemAuthor::system("contract-organizer"),
        "contract-area-merge",
    );
    let result = store
        .merge_area(&ctx, "contract-source", "contract-destination", false, None)
        .unwrap();
    assert_eq!(result.item_ids, vec![id.clone()]);
    let got = store.get(&id).unwrap().unwrap();
    assert_eq!(got.area.as_deref(), Some("contract-destination"));
    assert_eq!(got.author, author.author);
    let history = store.get_item_history(&id).unwrap();
    assert_eq!(history.last().unwrap().author, ctx.author);
    assert_eq!(history.last().unwrap().changed_by, "contract-area-merge");
    assert_eq!(
        store.restore_area(&ctx, &id, None).unwrap().as_deref(),
        Some("contract-source")
    );
}

pub fn archive(store: &dyn MemStore) {
    let ctx = WriteContext::new(MemAuthor::owner(None), "contract-create");
    let id = store
        .save(&ctx, &item("contract archive"), false)
        .unwrap()
        .id;
    let output = store
        .archive(&ctx, &id, "contract obsolete", "contract-archive")
        .unwrap();
    assert_eq!(output["previous_kind"], "fact");
    let got = store.get(&id).unwrap().unwrap();
    assert_eq!(got.kind, "archived");
    assert_eq!(got.body, "contract astrolabe observations");
    let history = store.get_item_history(&id).unwrap();
    assert_eq!(history.last().unwrap().change_type, "archive");
    assert_eq!(history.last().unwrap().author, ctx.author);
    assert_eq!(history.last().unwrap().changed_by, "contract-archive");
    let mut pinned = item("contract pinned");
    pinned.pinned = true;
    let id = store.save(&ctx, &pinned, false).unwrap().id;
    assert_eq!(
        store
            .archive(&ctx, &id, "refused", "contract")
            .unwrap_err()
            .status_code(),
        409
    );
    assert_eq!(store.get(&id).unwrap().unwrap().kind, "fact");
}

pub fn guarded_replace_is_atomic(store: &dyn MemStore) {
    let ctx = WriteContext::new(MemAuthor::owner(None), "contract-owner");
    let id = store
        .save(&ctx, &item("contract guarded"), false)
        .unwrap()
        .id;
    let mut guard = WriteContext::new(
        MemAuthor::neighbor("friend".into(), "friendship".into()),
        "contract-refused",
    );
    guard.replace_guard = Some(ReplaceGuard::new(vec!["contract-allowed".into()]));
    let mut replacement = item("contract guarded");
    replacement.area = Some("contract-allowed".into());
    replacement.body = "untrusted replacement".into();
    assert!(matches!(
        store.save(&guard, &replacement, true),
        Err(SaveError::Guard(ReplaceGuardError::TitleConflict))
    ));
    let got = store.get(&id).unwrap().unwrap();
    assert_eq!(got.body, "contract astrolabe observations");
    assert_eq!(got.area.as_deref(), Some("contract-source"));
    assert_eq!(got.author, ctx.author);
    assert_eq!(store.get_item_history(&id).unwrap().len(), 1);
}

pub fn export_import_round_trip(source: &dyn MemStore, target: &dyn MemStore) {
    let ctx = WriteContext::new(
        MemAuthor::owner(Some("migration-owner".into())),
        "migration",
    );
    let mut payload = item("contract migration");
    payload.body = "large contract blob ".repeat(200);
    let id = source.save(&ctx, &payload, false).unwrap().id;
    source.set_pinned_position(&ctx, &id, 1.5, -2.0).unwrap();
    source.graph_alias(&ctx, "migration-alias", &id).unwrap();
    source
        .graph_create_entity(
            &ctx,
            &crate::Entity {
                id: "migration-entity".into(),
                entity_type: "Concept".into(),
                scope: "global".into(),
                properties: serde_json::json!({"name":"migration"}),
            },
        )
        .unwrap();
    source
        .graph_relate(
            &ctx,
            &Relation {
                from_id: id.clone(),
                relation_type: "mentions".into(),
                to_id: "migration-entity".into(),
                scope: "global".into(),
                valid_from: "2026-01-01T00:00:00Z".into(),
                valid_to: None,
                confidence: 1.0,
                properties: serde_json::json!({}),
                reason: None,
            },
        )
        .unwrap();
    let identity = EmbedderIdentity {
        provider: "contract".into(),
        model: "migration".into(),
        dimension: 3,
    };
    source.init_vectors(&ctx, &identity).unwrap();
    let vector = StampedVector {
        identity: identity.clone(),
        vector: vec![1.0, 0.0, 0.0],
    };
    source.insert_vector(&ctx, &id, 0, &vector).unwrap();
    let pages = export_all(source);
    let importer = WriteContext::system("must-not-restamp");
    for page in &pages {
        target.import_page(&importer, page).unwrap();
    }
    assert_eq!(
        target.table_counts().unwrap(),
        source.table_counts().unwrap()
    );
    assert_eq!(export_all(target), pages);
    assert_eq!(target.vector_stamp().unwrap(), Some(identity));
    assert_eq!(target.vector_search(&vector, 1).unwrap()[0].0, id);
    assert_eq!(target.managed_payload(&id).unwrap().unwrap().0.body, payload.body);
}

fn export_all(store: &dyn MemStore) -> Vec<crate::backend::ExportPage> {
    let mut cursor = None;
    let mut pages = Vec::new();
    loop {
        let page = store.export_page(cursor.as_ref(), 1).unwrap();
        cursor = page.next.clone();
        pages.push(page);
        if cursor.is_none() {
            return pages;
        }
    }
}

#[cfg(test)]
mod tests;

mod engines;
pub use engines::{engine_auxiliary_round_trip, engine_pending_contract, engine_shadow_contract};
