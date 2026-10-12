use super::*;

#[test]
fn directives_keep_source_paths_and_skip_literal_examples() {
    let source = "Lead\r\n@ds-image \"C:\\work\\a_b [1].png\"\r\n@ct-document '/work/note.md'\n```text\n@ds-page /example.html\n```\n    @ds-image /indented.png\n";
    let plan = directives(source);
    assert_eq!(plan.len(), 2);
    assert_eq!(plan[0].path, "C:\\work\\a_b [1].png");
    assert_eq!(plan[1].path, "/work/note.md");
    assert_eq!(
        &source[plan[0].start..plan[0].end],
        "@ds-image \"C:\\work\\a_b [1].png\""
    );
}

#[test]
fn transferred_paths_replace_only_directives_and_preserve_prose_and_line_endings() {
    let source = "Lead /origin/a.png\r\n@ds-image /origin/a.png\r\n@ct-image /origin/a.png\nTail";
    let paths = HashMap::from([("/origin/a.png".into(), "C:\\target\\image.png".into())]);
    let rebound = rewrite(source, &directives(source), &paths).unwrap();
    assert_eq!(rebound, "Lead /origin/a.png\r\n@ds-image \"C:\\target\\image.png\"\r\n@ct-image \"C:\\target\\image.png\"\nTail");
    assert_eq!(
        directives(&rebound)
            .iter()
            .map(|directive| directive.path.as_str())
            .collect::<Vec<_>>(),
        ["C:\\target\\image.png", "C:\\target\\image.png"]
    );
}

#[test]
fn missing_or_malformed_transfer_paths_never_produce_a_deliverable_body() {
    let source = "@ds-document /origin/note.md";
    let plan = directives(source);
    assert!(rewrite(source, &plan, &HashMap::new()).is_err());
    for target in ["", "/target/\"quoted.md", "/target/line\nbreak.md"] {
        let paths = HashMap::from([("/origin/note.md".into(), target.into())]);
        assert!(rewrite(source, &plan, &paths).is_err());
    }
}

#[test]
fn ingest_targets_the_same_tab_and_only_message_routes() {
    assert_eq!(
        ingest_path("POST", "/api/tabs/main/send?x=1"),
        Some("/api/tabs/main/attachments".into())
    );
    assert_eq!(
        ingest_path("post", "/api/tabs/a%20b/chat-send"),
        Some("/api/tabs/a%20b/attachments".into())
    );
    for (method, path) in [
        ("GET", "/api/tabs/main/send"),
        ("POST", "/api/tabs//send"),
        ("POST", "/api/tabs/main/preview/read"),
        ("POST", "/api/agents/spawn"),
    ] {
        assert_eq!(ingest_path(method, path), None);
    }
}

#[tokio::test]
async fn transfer_rebinds_text_and_batch_to_one_target_copy() {
    let mut body = serde_json::json!({ "text": "@ds-image /origin/a.png", "batch": [{"text": "@ct-document /origin/a.png"}] });
    let mut target_files = HashMap::new();
    let mut calls = Vec::new();
    transfer_with(
        "POST",
        "/api/tabs/target/send",
        &mut body,
        |source, route| {
            calls.push((source.clone(), route));
            let target = "/target/uploads/a.png".to_string();
            target_files.insert(target.clone(), b"fixture-image".to_vec());
            std::future::ready(Ok(target))
        },
    )
    .await
    .unwrap();
    assert_eq!(
        calls,
        [(
            "/origin/a.png".into(),
            "/api/tabs/target/attachments".into()
        )]
    );
    for text in [
        body["text"].as_str().unwrap(),
        body["batch"][0]["text"].as_str().unwrap(),
    ] {
        let path = &directives(text)[0].path;
        assert_eq!(target_files.get(path).unwrap(), b"fixture-image");
        assert!(!text.contains("/origin/"));
    }
}

#[tokio::test]
async fn failed_or_over_bound_transfer_preserves_the_unsent_body() {
    let mut body = serde_json::json!({"text": "@ds-image /origin/a.png"});
    let original = body.clone();
    assert!(
        transfer_with("POST", "/api/tabs/target/send", &mut body, |_, _| {
            std::future::ready(Err("transfer refused".into()))
        })
        .await
        .is_err()
    );
    assert_eq!(body, original);
    let mut body = serde_json::json!({"text": "@ds-image /origin/a.png\n".repeat(33)});
    let original = body.clone();
    let calls = std::cell::Cell::new(0);
    assert!(
        transfer_with("POST", "/api/tabs/target/send", &mut body, |_, _| {
            calls.set(calls.get() + 1);
            std::future::ready(Ok("/target/a.png".into()))
        })
        .await
        .is_err()
    );
    assert_eq!(body, original);
    assert_eq!(calls.get(), 0);
}
