use crate::{
    store::{with_author, MemAuthor, MemKind, MemScope},
    MemManager, NewMemItem,
};

#[test]
fn authorship_save_overwrites_in_both_directions_even_with_equal_body() {
    let owner = MemAuthor::owner(Some("owner".into()));
    let neighbor = MemAuthor::neighbor("friend".into(), "friendship".into());
    for (first, second) in [(owner.clone(), neighbor.clone()), (neighbor, owner)] {
        for replacement in ["replacement", "same body"] {
            let mem = MemManager::open_in_memory().unwrap();
            let item = |body: &str| NewMemItem {
                kind: MemKind::Fact,
                scope: MemScope::Global,
                title: "Shared fact".into(),
                summary: None,
                area: None,
                tags: vec![],
                body: body.into(),
                source: None,
                importance: 3.0,
                source_path: None,
                pinned: false,
            };
            let id = with_author(first.clone(), || mem.save(item("same body"))).unwrap();
            let result = with_author(second.clone(), || {
                mem.save_with_outcome(item(replacement), false)
            })
            .unwrap();
            assert_eq!(result.id, id);
            assert_eq!(result.outcome, "updated");
            let got = mem.get(&id).unwrap().unwrap();
            assert_eq!(got.author, second);
            assert_eq!(got.body, replacement);
            let hits = mem
                .search(&crate::search::SearchQuery {
                    query: "Shared fact".into(),
                    ..Default::default()
                })
                .unwrap();
            let hit = hits
                .iter()
                .find(|hit| hit.item.id == id)
                .expect("search returns the authored item");
            assert_eq!(
                serde_json::to_value(hit).unwrap()["item"]["author"],
                serde_json::to_value(&second).unwrap()
            );
            let history = mem.get_item_history(&id).unwrap();
            assert_eq!(history.len(), 2);
            assert_eq!(history[1].old_body.as_deref(), Some("same body"));
            assert_eq!(history[1].author, first);
        }
    }
}
