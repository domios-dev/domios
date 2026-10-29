use super::*;

fn item(body: &str) -> NewMemItem {
    NewMemItem {
        kind: MemKind::Fact,
        scope: MemScope::Global,
        title: "Shared title".into(),
        summary: None,
        area: None,
        tags: vec![],
        body: body.into(),
        source: None,
        importance: 3.0,
        source_path: None,
        pinned: false,
    }
}

#[test]
fn overwrite_history_keeps_prior_author_and_full_blob_body_in_both_directions() {
    let owner = MemAuthor::owner(Some("owner-uid".into()));
    let neighbor = MemAuthor::neighbor("friend-uid".into(), "friendship".into());
    for (first, second) in [(owner.clone(), neighbor.clone()), (neighbor, owner)] {
        let (conn, blobs) = fresh();
        let body = "original ".repeat(600);
        let id = with_author(first.clone(), || insert(&conn, &item(&body), &blobs)).unwrap();
        with_author(second.clone(), || {
            replace_existing(&conn, &id, &item("replacement"), &blobs)
        })
        .unwrap();
        let got = get(&conn, &id, &blobs).unwrap().unwrap();
        assert_eq!(got.author, second);
        let graph = graph_full(&conn).unwrap();
        let projected = graph.nodes.iter().find(|node| node.id == id).unwrap();
        assert_eq!(projected.author.as_ref(), Some(&second));
        assert_eq!(got.body, "replacement");
        let history = get_item_history(&conn, &id).unwrap();
        let last = history.last().unwrap();
        assert_eq!(last.old_body.as_deref(), Some(body.as_str()));
        assert_eq!(last.author, first);
        assert_eq!(
            serde_json::to_value(got).unwrap()["author"]["kind"],
            second.kind.as_str()
        );
    }
}

#[test]
fn unscoped_writes_are_explicit_system_and_nested_scopes_restore() {
    let (conn, blobs) = fresh();
    let id = insert(&conn, &item("unscoped"), &blobs).unwrap();
    assert_eq!(
        get(&conn, &id, &blobs).unwrap().unwrap().author,
        MemAuthor::system("unscoped")
    );
    with_author(MemAuthor::owner(None), || {
        assert_eq!(current_author(), MemAuthor::owner(None));
        with_author(MemAuthor::system("nested"), || {
            assert_eq!(current_author(), MemAuthor::system("nested"));
        });
        assert_eq!(current_author(), MemAuthor::owner(None));
    });
    assert_eq!(current_author(), MemAuthor::system("unscoped"));
}

#[test]
fn author_json_is_structured_and_omits_absent_identity_fields() {
    assert_eq!(
        serde_json::to_value(MemAuthor::owner(None)).unwrap(),
        serde_json::json!({"kind":"owner"})
    );
    assert_eq!(
        serde_json::to_value(MemAuthor::neighbor("uid".into(), "friendship".into())).unwrap(),
        serde_json::json!({"kind":"neighbor","account_uid":"uid","friendship_id":"friendship"})
    );
    assert_eq!(
        serde_json::to_value(MemAuthor::system("calibration")).unwrap(),
        serde_json::json!({"kind":"system","label":"calibration"})
    );
}
