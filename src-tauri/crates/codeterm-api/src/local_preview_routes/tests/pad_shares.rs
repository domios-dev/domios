use super::*;

#[tokio::test]
async fn tab_send_grants_an_exact_canvas_and_its_items_to_owner_chat() {
    let fixture = Fixture::new();
    let sender = &fixture.sibling;
    let recipient = &fixture.owner;
    let pad = fixture.pad(sender);
    std::fs::create_dir_all(&pad).unwrap();
    let board = pad.join("shared.canvas.json");
    let item = pad.join("item.md");
    let ungranted = pad.join("other.md");
    std::fs::write(&item, "shared item").unwrap();
    std::fs::write(&ungranted, "private item").unwrap();
    let contents = |item| {
        format!(r#"{{"format":"domios.canvas/1","items":[{{"kind":"document","src":{item}}}]}}"#)
    };
    let original = contents(serde_json::json!(item));
    std::fs::write(&board, &original).unwrap();
    fixture.reference(recipient, &board);
    fixture.reference(recipient, &ungranted);
    assert_eq!(
        fixture.read(recipient, &board, "canvas").await.status(),
        StatusCode::NOT_FOUND
    );

    let card = format!("<ds-canvas src=\"{}\" />", board.display());
    let response = fixture
        .send(sender, recipient, serde_json::json!({"text": card}))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json(response).await["ok"], true);

    let read_board = fixture.read(recipient, &board, "canvas").await;
    assert_eq!(read_board.status(), StatusCode::OK);
    assert_eq!(json(read_board).await["data"], original);
    let item_read = serde_json::json!({"path": item, "kind":"document", "canvas":board});
    let owner = Some("Bearer preview-test-token");
    let response = fixture.post(recipient, "read", item_read, owner).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json(response).await["data"], "shared item");
    assert_eq!(
        fixture
            .read(recipient, &ungranted, "document")
            .await
            .status(),
        StatusCode::NOT_FOUND
    );

    let tab_token = fixture
        .state
        .svc
        .mint_tab_credential(recipient, "share-nonce")
        .unwrap();
    let read = serde_json::json!({"path": board, "kind": "canvas"});
    assert_eq!(
        fixture
            .post(
                recipient,
                "read",
                read.clone(),
                Some(&format!("Bearer {tab_token}"))
            )
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let peer_key = fixture.isolated_peer();
    for (origin, status) in [
        (AuthenticatedOrigin::Owner, StatusCode::OK),
        (AuthenticatedOrigin::Mesh, StatusCode::NOT_FOUND),
    ] {
        let token = codeterm_service_core::mesh::crown::sign_request_with_origin(
            &peer_key,
            "preview-peer",
            origin,
        )
        .unwrap();
        assert_eq!(
            fixture
                .post(
                    recipient,
                    "read",
                    read.clone(),
                    Some(&format!("Bearer {token}"))
                )
                .await
                .status(),
            status
        );
    }
    let updated = contents(serde_json::json!(ungranted));
    std::fs::write(&board, &updated).unwrap();
    let read_ungranted = serde_json::json!({"path": ungranted, "kind":"document", "canvas":board});
    assert_eq!(
        fixture
            .post(recipient, "read", read_ungranted, owner)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let write = serde_json::json!({"path": board, "base": updated, "document": original});
    assert_eq!(
        fixture
            .post(recipient, "canvas/write", write, owner)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );

    fixture.state.svc.close_tab_async(sender).await.unwrap();
    assert_eq!(
        fixture.read(recipient, &board, "canvas").await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn copy_attachment_snapshots_canvas_items_in_the_recipient_inbox() {
    let fixture = Fixture::new();
    let sender = &fixture.sibling;
    let recipient = &fixture.owner;
    let source = fixture.pad(sender);
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(fixture.pad(recipient)).unwrap();
    let item = source.join("item.md");
    let board = source.join("board.canvas.json");
    std::fs::write(&item, "before copying").unwrap();
    let content = format!(
        r#"{{"format":"domios.canvas/1","items":[{{"kind":"document","src":{}}}]}}"#,
        serde_json::json!(item),
    );
    std::fs::write(&board, content).unwrap();
    let refusal = fixture
        .send(
            sender,
            recipient,
            serde_json::json!({
                "attach": ["board.canvas.json", "../foreign.md"], "copy": true
            }),
        )
        .await;
    assert_eq!(refusal.status(), StatusCode::CONFLICT);
    assert_eq!(json(refusal).await["error"]["bytes_written"], false);
    let inbox = fixture.pad(recipient).join("inbox");
    assert!(!inbox.exists());
    let response = fixture
        .send(
            sender,
            recipient,
            serde_json::json!({
                "attach": ["board.canvas.json"], "copy": true
            }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json(response).await["ok"], true);
    let files: Vec<_> = std::fs::read_dir(&inbox)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(files.len(), 2);
    let copied = files
        .iter()
        .find(|file| file.to_string_lossy().ends_with(".canvas.json"))
        .unwrap();
    let copy = std::fs::read_to_string(copied).unwrap();
    let child: serde_json::Value = serde_json::from_str(&copy).unwrap();
    let child = PathBuf::from(child["items"][0]["src"].as_str().unwrap());
    assert!(files
        .iter()
        .any(|file| file.canonicalize().unwrap() == child.canonicalize().unwrap()));
    assert_eq!(std::fs::read_to_string(&child).unwrap(), "before copying");
    assert_eq!(
        fixture.read(recipient, &board, "canvas").await.status(),
        StatusCode::NOT_FOUND
    );
    fixture.state.svc.close_tab_async(sender).await.unwrap();
    fixture.reference(recipient, copied);
    assert_eq!(
        fixture.read(recipient, copied, "canvas").await.status(),
        StatusCode::OK
    );
}
