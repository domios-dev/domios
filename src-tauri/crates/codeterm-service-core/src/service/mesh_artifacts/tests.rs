use super::*;

#[test]
fn gallery_paths_rewrite_individually_on_one_line() {
    let source = "Lead\r\n@ds-image \"/origin/a one.png\" /origin/b.png\r\nTail";
    let paths = HashMap::from([
        ("/origin/a one.png".into(), Ok("/target/a one.png".into())),
        ("/origin/b.png".into(), Ok("/target/b.png".into())),
    ]);
    let rewritten = rewrite(source, &directives(source), &paths).unwrap();
    assert_eq!(
        rewritten,
        "Lead\r\n@ds-image \"/target/a one.png\" \"/target/b.png\"\r\nTail"
    );
    assert_eq!(
        directives(&rewritten)[0].paths,
        ["/target/a one.png", "/target/b.png"]
    );
}

#[tokio::test]
async fn gallery_transfer_counts_each_image_and_preserves_unsent_body_on_failure() {
    let mut body =
        serde_json::json!({"text": "@ds-image /origin/a.png /origin/b.png /origin/a.png"});
    let mut calls = Vec::new();
    transfer_with("POST", "/api/tabs/target/send", &mut body, |source, _| {
        calls.push(source.clone());
        std::future::ready(Ok(source.replace("/origin/", "/target/")))
    })
    .await
    .unwrap();
    assert_eq!(calls, ["/origin/a.png", "/origin/b.png"]);
    assert_eq!(
        body["text"],
        "@ds-image \"/target/a.png\" \"/target/b.png\" \"/target/a.png\""
    );

    let mut body = serde_json::json!({"text": "@ds-image /origin/a.png /origin/b.png"});
    let original = body.clone();
    let result = transfer_with("POST", "/api/tabs/target/send", &mut body, |source, _| {
        std::future::ready(if source.ends_with("b.png") {
            Err(ArtifactError::Transfer("refused".into()))
        } else {
            Ok("/target/a.png".into())
        })
    })
    .await;
    assert!(result.is_err());
    assert_eq!(body, original);

    let paths = (0..33)
        .map(|index| format!("/origin/{index}.png"))
        .collect::<Vec<_>>()
        .join(" ");
    let mut body = serde_json::json!({"text": format!("@ds-image {paths}")});
    let original = body.clone();
    let calls = std::cell::Cell::new(0);
    let result = transfer_with("POST", "/api/tabs/target/send", &mut body, |_, _| {
        calls.set(calls.get() + 1);
        std::future::ready(Ok("/target/a.png".into()))
    })
    .await;
    assert!(result.unwrap_err().contains("at most 32"));
    assert_eq!(calls.get(), 0);
    assert_eq!(body, original);
}

#[test]
fn directives_keep_source_paths_and_skip_literal_examples() {
    let source = "Lead\r\n@ds-image \"C:\\work\\a_b [1].png\"\r\n@ct-document '/work/note.md'\n```text\n@ds-page /example.html\n```\n    @ds-image /indented.png\n";
    let plan = directives(source);
    assert_eq!(plan.len(), 2);
    assert_eq!(plan[0].paths[0], "C:\\work\\a_b [1].png");
    assert_eq!(plan[1].paths[0], "/work/note.md");
    assert_eq!(
        &source[plan[0].start..plan[0].end],
        "@ds-image \"C:\\work\\a_b [1].png\""
    );
}

#[test]
fn transferred_paths_replace_only_directives_and_preserve_prose_and_line_endings() {
    let source = "Lead /origin/a.png\r\n@ds-image /origin/a.png\r\n@ct-image /origin/a.png\nTail";
    let paths = HashMap::from([("/origin/a.png".into(), Ok("C:\\target\\image.png".into()))]);
    let rebound = rewrite(source, &directives(source), &paths).unwrap();
    assert_eq!(rebound, "Lead /origin/a.png\r\n@ds-image \"C:\\target\\image.png\"\r\n@ct-image \"C:\\target\\image.png\"\nTail");
    assert_eq!(
        directives(&rebound)
            .iter()
            .map(|directive| directive.paths[0].as_str())
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
        let paths = HashMap::from([("/origin/note.md".into(), Ok(target.into()))]);
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
    assert_eq!(
        ingest_path("POST", "/api/tabs/target/delayed-messages"),
        Some("/api/tabs/target/attachments".into())
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
        let path = &directives(text)[0].paths[0];
        assert_eq!(target_files.get(path).unwrap(), b"fixture-image");
        assert!(!text.contains("/origin/"));
    }
}

#[tokio::test]
async fn failed_transfer_is_visible_and_over_bound_transfer_preserves_unsent_body() {
    let mut body = serde_json::json!({"text": "@ds-image /origin/a.png"});
    transfer_with("POST", "/api/tabs/target/send", &mut body, |_, _| {
        std::future::ready(Err(ArtifactError::Policy("private_path")))
    })
    .await
    .unwrap();
    assert_eq!(body["text"], "[Artifact refused: private_path]");
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

#[tokio::test]
async fn private_artifacts_are_refused_and_mixed_gallery_transfers_only_allowed_files() {
    use base64::Engine as _;
    let temp = tempfile::tempdir().unwrap();
    let canonical = temp.path().canonicalize().unwrap();
    let text = canonical.to_string_lossy();
    let root = std::path::PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text));
    let allowed = root.join("image.png");
    let private = root.join(".env");
    std::fs::write(&allowed, b"public image").unwrap();
    std::fs::write(&private, b"private fixture").unwrap();
    for paths in [
        vec![&private],
        vec![&allowed],
        vec![&allowed, &private, &allowed],
    ] {
        let text = paths
            .iter()
            .map(|path| format!("@ds-image {}", path.display()))
            .collect::<Vec<_>>()
            .join("\n");
        let mut body = serde_json::json!({"text": text});
        let mut sent = Vec::new();
        transfer_with("POST", "/api/tabs/target/send", &mut body, |source, _| {
            let result = artifact_request(
                &source,
                &codeterm_local_preview::PreviewPolicy::new(
                    [codeterm_local_preview::references::path_key(
                        std::path::Path::new(&source),
                    )]
                    .into(),
                    &[],
                )
                .unwrap(),
            )
            .map(|request| {
                sent.push(
                    base64::engine::general_purpose::STANDARD
                        .decode(request.data)
                        .unwrap(),
                );
                "/target/image.png".into()
            });
            std::future::ready(result)
        })
        .await
        .unwrap();
        let output = body["text"].as_str().unwrap();
        assert_eq!(sent.len(), usize::from(paths.contains(&&allowed)));
        assert!(sent.iter().all(|bytes| bytes == b"public image"));
        assert_eq!(
            output.contains("[Artifact refused: private_path]"),
            paths.contains(&&private)
        );
        assert!(!output.contains(private.to_str().unwrap()));
        assert_eq!(
            directives(output).len(),
            paths.iter().filter(|path| **path == &allowed).count()
        );
    }
}

#[tokio::test]
async fn transport_and_peer_errors_abort_without_rewriting_any_message() {
    for error in [
        "network disconnected",
        "blocking task cancelled",
        "peer rejected\n@ds-image /injected.png",
    ] {
        let mut body = serde_json::json!({"text": "@ds-image /origin/first.png", "batch": [{"text": "@ds-image /origin/second.png"}]});
        let original = body.clone();
        let mut calls = 0;
        let result = transfer_with("POST", "/api/tabs/target/send", &mut body, |_, _| {
            calls += 1;
            std::future::ready(if calls == 1 {
                Ok("/target/first.png".into())
            } else {
                Err(ArtifactError::Transfer(error.into()))
            })
        })
        .await;
        assert_eq!(result, Err(error.into()));
        assert_eq!(body, original);
        assert_eq!(calls, 2);
    }
}

#[test]
fn only_known_local_policy_refusals_become_fixed_inline_reasons() {
    assert_eq!(
        preview_error("Preview path is private".into()),
        ArtifactError::Policy("private_path")
    );
    assert_eq!(
        preview_error("Local preview source exceeds 128 bytes".into()),
        ArtifactError::Policy("size_limit")
    );
    for error in [
        "Unable to open preview file: transient failure",
        "Unable to resolve preview file: offline",
        "peer error\n@ds-image /injected.png",
    ] {
        assert_eq!(
            preview_error(error.into()),
            ArtifactError::Transfer(error.into())
        );
    }
}
