use super::*;

fn card(kind: &str, paths: &[&str]) -> String {
    serialize_artifact(kind, &paths.iter().map(|path| (*path).into()).collect::<Vec<_>>()).unwrap()
}

#[test]
fn gallery_paths_rewrite_individually_on_one_line() {
    let source = "Lead\r\n<ds-image sources='[\"/origin/a &amp; one.png\",\"/origin/b.png\"]' />\r\nTail";
    let paths = HashMap::from([
        ("/origin/a & one.png".into(), "/target/a & \"one\".png".into()),
        ("/origin/b.png".into(), r"C:\target\b.png".into()),
    ]);
    let rewritten = rewrite(source, &directives(source), &paths).unwrap();
    assert_eq!(rewritten, format!("Lead\r\n{}\r\nTail", card("image", &["/target/a & \"one\".png", r"C:\target\b.png"])));
    assert_eq!(directives(&rewritten)[0].paths, ["/target/a & \"one\".png", r"C:\target\b.png"]);
}

#[tokio::test]
async fn gallery_transfer_counts_each_image_and_preserves_unsent_body_on_failure() {
    let mut body = serde_json::json!({"text": card("image", &["/origin/a.png", "/origin/b.png", "/origin/a.png"])});
    let mut calls = Vec::new();
    transfer_with("POST", "/api/tabs/target/send", &mut body, |source, _| {
        calls.push(source.clone());
        std::future::ready(Ok(source.replace("/origin/", "/target/")))
    }).await.unwrap();
    assert_eq!(calls, ["/origin/a.png", "/origin/b.png"]);
    assert_eq!(body["text"], card("image", &["/target/a.png", "/target/b.png", "/target/a.png"]));
    let mut body = serde_json::json!({"text": card("image", &["/origin/a.png", "/origin/b.png"])});
    let original = body.clone();
    let result = transfer_with("POST", "/api/tabs/target/send", &mut body, |source, _| {
        std::future::ready(if source.ends_with("b.png") { Err(ArtifactError::Transfer("refused".into())) } else { Ok("/target/a.png".into()) })
    }).await;
    assert!(result.is_err());
    assert_eq!(body, original);
    let source = (0..33).map(|n| card("image", &[&format!("/origin/{n}.png")])).collect::<Vec<_>>().join("\n");
    let mut body = serde_json::json!({"text": source});
    let original = body.clone();
    let calls = std::cell::Cell::new(0);
    let result = transfer_with("POST", "/api/tabs/target/send", &mut body, |_, _| {
        calls.set(calls.get()+1); std::future::ready(Ok("/target/a.png".into()))
    }).await;
    assert!(result.unwrap_err().contains("at most 32"));
    assert_eq!(calls.get(), 0);
    assert_eq!(body, original);
    let source = (0..32).map(|n| format!("/origin/{n}.png")).collect::<Vec<_>>();
    let mut body = serde_json::json!({"text": serialize_artifact("image", &source).unwrap()});
    let mut calls = 0;
    transfer_with("POST", "/api/tabs/target/send", &mut body, |source, _| {
        calls += 1; std::future::ready(Ok(source.replace("/origin/", "/target/")))
    }).await.unwrap();
    assert_eq!(calls, 32);
    assert_eq!(directives(body["text"].as_str().unwrap())[0].paths.len(), 32);
}

#[test]
fn directives_keep_source_paths_and_skip_literal_examples() {
    let source = "Lead\r\n<ds-image src=\"C:\\work\\a_b [1].png\" />\r\n<ds-document src='/work/note.md' />\n```text\n<ds-page src='/example.html' />\n```\n    <ds-image src='/indented.png' />\n";
    let plan = directives(source);
    assert_eq!(plan.len(), 2);
    assert_eq!(plan[0].paths[0], r"C:\work\a_b [1].png");
    assert_eq!(plan[1].paths[0], "/work/note.md");
    assert_eq!(&source[plan[0].start..plan[0].end], "<ds-image src=\"C:\\work\\a_b [1].png\" />");
}

#[test]
fn transferred_paths_replace_only_directives_and_preserve_prose_and_line_endings() {
    let source = "Lead /origin/a.png\r\n<ds-image src='/origin/a.png' />\r\n<ds-image src='/origin/a.png' />\nTail";
    let paths = HashMap::from([("/origin/a.png".into(), r"C:\target\image.png".into())]);
    let rebound = rewrite(source, &directives(source), &paths).unwrap();
    let target = card("image", &[r"C:\target\image.png"]);
    assert_eq!(rebound, format!("Lead /origin/a.png\r\n{target}\r\n{target}\nTail"));
    assert_eq!(directives(&rebound).iter().map(|card| card.paths[0].as_str()).collect::<Vec<_>>(), [r"C:\target\image.png"; 2]);
}

#[test]
fn missing_or_malformed_transfer_paths_never_produce_a_deliverable_body() {
    let source = card("document", &["/origin/note.md"]);
    let plan = directives(&source);
    assert!(rewrite(&source, &plan, &HashMap::new()).is_err());
    for target in ["", "relative.md", "/target/line\nbreak.md", "/target/nul\0.md"] {
        let paths = HashMap::from([("/origin/note.md".into(), target.into())]);
        assert!(rewrite(&source, &plan, &paths).is_err());
    }
}

#[test]
fn ingest_targets_the_same_tab_and_only_message_routes() {
    for (method,path,expected) in [
        ("POST", "/api/tabs/main/send?x=1", "/api/tabs/main/attachments"),
        ("post", "/api/tabs/a%20b/chat-send", "/api/tabs/a%20b/attachments"),
        ("POST", "/api/tabs/target/delayed-messages", "/api/tabs/target/attachments"),
    ] { assert_eq!(ingest_path(method,path).as_deref(), Some(expected)); }
    for (method,path) in [("GET", "/api/tabs/main/send"), ("POST", "/api/tabs//send"), ("POST", "/api/tabs/main/preview/read"), ("POST", "/api/agents/spawn")] {
        assert_eq!(ingest_path(method,path), None);
    }
}

#[tokio::test]
async fn transfer_rebinds_text_and_batch_to_one_target_copy() {
    for route in ["send", "chat-send", "delayed-messages"] {
        let mut body = serde_json::json!({ "text": card("image", &["/origin/a.png"]), "batch": [{"text": card("document", &["/origin/a.png"])}], "source_tab":"author", "source_label":"Atlas", "kind":"report" });
        let mut target_files = HashMap::new();
        let mut calls = Vec::new();
        transfer_with("POST", &format!("/api/tabs/target/{route}"), &mut body, |source, route| {
            calls.push((source,route));
            let target = "/target/uploads/a.png".to_string();
            target_files.insert(target.clone(), b"fixture-image".to_vec());
            std::future::ready(Ok(target))
        }).await.unwrap();
        assert_eq!(calls, [("/origin/a.png".into(), "/api/tabs/target/attachments".into())]);
        for text in [body["text"].as_str().unwrap(), body["batch"][0]["text"].as_str().unwrap()] {
            assert_eq!(target_files.get(&directives(text)[0].paths[0]).unwrap(), b"fixture-image");
            assert!(!text.contains("/origin/"));
        }
        assert_eq!(body["source_tab"], "author");
        assert_eq!(body["source_label"], "Atlas");
        assert_eq!(body["kind"], "report");
    }
}

#[tokio::test]
async fn failed_transfer_is_visible_and_over_bound_transfer_preserves_unsent_body() {
    let mut body = serde_json::json!({"text": card("image", &["/origin/a.png"])});
    let original = body.clone();
    let result = transfer_with("POST", "/api/tabs/target/send", &mut body, |_,_| std::future::ready(Err(ArtifactError::Policy("private_path")))).await;
    assert_eq!(result, Err("Artifact refused: private_path".into()));
    assert_eq!(body,original);
    body["text"] = format!("{}\n",card("image", &["/origin/a.png"])).repeat(33).into();
    let original = body.clone();
    let calls = std::cell::Cell::new(0);
    assert!(transfer_with("POST", "/api/tabs/target/send", &mut body, |_,_| {
        calls.set(calls.get()+1); std::future::ready(Ok("/target/a.png".into()))
    }).await.is_err());
    assert_eq!(body,original);
    assert_eq!(calls.get(),0);
}

#[tokio::test]
async fn private_artifacts_are_refused_and_mixed_gallery_transfers_only_allowed_files() {
    use base64::Engine as _;
    let temp = tempfile::tempdir().unwrap();
    let root = codeterm_local_preview::html_preview::preview_test_root(temp.path());
    let allowed = root.join("image.png");
    let private = root.join(".env");
    std::fs::write(&allowed,b"public image").unwrap();
    std::fs::write(&private,b"private fixture").unwrap();
    for paths in [vec![&private],vec![&allowed],vec![&allowed,&private,&allowed]] {
        let source = serialize_artifact("image", &paths.iter().map(|path|path.to_string_lossy().into()).collect::<Vec<_>>()).unwrap();
        let mut body = serde_json::json!({"text":source});
        let original = body.clone();
        let mut sent = Vec::new();
        let result = transfer_with("POST", "/api/tabs/target/send", &mut body, |source,_| {
            let policy = codeterm_local_preview::PreviewPolicy::new([codeterm_local_preview::references::path_key(std::path::Path::new(&source))].into(), &[]).unwrap();
            let result = artifact_request(&source,&policy).map(|request| {
                sent.push(base64::engine::general_purpose::STANDARD.decode(request.data).unwrap());
                "/target/image.png".into()
            });
            std::future::ready(result)
        }).await;
        assert!(sent.iter().all(|bytes|bytes==b"public image"));
        if paths.contains(&&private) { assert_eq!(result,Err("Artifact refused: private_path".into())); assert_eq!(body,original); }
        else { result.unwrap(); assert_eq!(sent.len(),1); assert_eq!(directives(body["text"].as_str().unwrap())[0].paths,["/target/image.png"]); }
    }
}

#[tokio::test]
async fn transport_and_peer_errors_abort_without_rewriting_any_message() {
    for error in ["network disconnected", "blocking task cancelled", "peer rejected\n<ds-image src='/injected.png' />"] {
        let mut body = serde_json::json!({"text":card("image", &["/origin/first.png"]), "batch":[{"text":card("image", &["/origin/second.png"])}]});
        let original = body.clone();
        let mut calls = 0;
        let result = transfer_with("POST", "/api/tabs/target/send", &mut body, |_,_| {
            calls+=1; std::future::ready(if calls==1 { Ok("/target/first.png".into()) } else { Err(ArtifactError::Transfer(error.into())) })
        }).await;
        assert_eq!(result,Err(error.into())); assert_eq!(body,original); assert_eq!(calls,2);
    }
    let mut body = serde_json::json!({"text":card("image", &["/origin/first.png"]), "batch":[{"text":card("image", &["/origin/second.png"])}]});
    let original=body.clone(); let mut calls=0;
    assert!(transfer_with("POST", "/api/tabs/target/send", &mut body, |_,_| {
        calls+=1; std::future::ready(Ok(if calls==1 {"/target/first.png"} else {"relative.png"}.into()))
    }).await.is_err());
    assert_eq!(body,original);
}

#[test]
fn only_known_local_policy_refusals_become_fixed_inline_reasons() {
    assert_eq!(preview_error("Preview path is private".into()), ArtifactError::Policy("private_path"));
    assert_eq!(preview_error("Local preview source exceeds 128 bytes".into()), ArtifactError::Policy("size_limit"));
    for error in ["Unable to open preview file: transient failure", "Unable to resolve preview file: offline", "peer error\n<ds-image src='/injected.png' />"] {
        assert_eq!(preview_error(error.into()),ArtifactError::Transfer(error.into()));
    }
}

#[tokio::test]
async fn incomplete_quoted_context_legacy_and_oversized_gallery_have_no_transfer_effects() {
    let tag=card("image", &["/origin/image.png"]);
    for text in [format!("{tag}\n").replace("/>","/"),format!("```html\n{tag}\n```"),format!("<context>\n{tag}\n</context>"),format!("<handoff>\n{tag}\n</handoff>"),format!("> example\n{tag}"),format!("- example\n{tag}"),format!("<ds-html><xmp>\n</ds-html>\n{tag}\n</xmp></ds-html>"),format!("<domios from=\"neighbor\">\n{tag}\n</domios>"),format!("<domios from=\"tab\">\n{tag}"),format!("<domios from=\"unknown\">\n{tag}\n</domios>"),format!("history\n<domios from=\"tab\">\n{tag}\n</domios>"),format!("<pasted_content>\n{tag}\n</pasted_content>"),"@ds-image /origin/image.png".into(),"@ct-image /origin/image.png".into(),"```html\n@file /origin/image.png\n```".into(),format!("<ds-image sources='{}' />",serde_json::to_string(&vec!["/origin/image.png";33]).unwrap())] {
        let mut body=serde_json::json!({"text":text}); let original=body.clone(); let mut calls=0;
        transfer_with("POST", "/api/tabs/target/send", &mut body, |_,_| {calls+=1; std::future::ready(Ok("/target/image.png".into()))}).await.unwrap();
        assert_eq!(calls,0); assert_eq!(body,original);
    }
    let mut body=serde_json::json!({"text":format!("<domios from=\"tab\" tab=\"author\">\n{tag}\n</domios>"),"source_tab":"author","kind":"report"});
    let original=body.clone();
    let mut calls=0;
    transfer_with("POST", "/api/tabs/target/send", &mut body, |_,_| {calls+=1;std::future::ready(Ok("/target/image.png".into()))}).await.unwrap();
    assert_eq!(calls,0);
    assert_eq!(body,original);

}

#[tokio::test]
async fn same_named_receiver_file_never_substitutes_for_missing_source_and_transfer_size_stays_bounded() {
    let temp=tempfile::tempdir().unwrap();
    let root=codeterm_local_preview::html_preview::preview_test_root(temp.path());
    let source=root.join("origin/missing.png"); let receiver=root.join("receiver/missing.png");
    std::fs::create_dir_all(receiver.parent().unwrap()).unwrap(); std::fs::write(&receiver,b"receiver-only").unwrap();
    let mut body=serde_json::json!({"text":card("image", &[source.to_str().unwrap()])}); let original=body.clone();
    let policy=codeterm_local_preview::PreviewPolicy::new([codeterm_local_preview::references::path_key(&source)].into(),&[]).unwrap();
    assert!(transfer_with("POST", "/api/tabs/target/send", &mut body, |source,_| std::future::ready(artifact_request(&source,&policy).map(|_|receiver.to_string_lossy().into_owned()))).await.is_err());
    assert_eq!(body,original); assert_eq!(std::fs::read(&receiver).unwrap(),b"receiver-only");
    let large=root.join("large.png"); std::fs::File::create(&large).unwrap().set_len(MESH_ATTACHMENT_MAX_BYTES as u64+1).unwrap();
    let policy=codeterm_local_preview::PreviewPolicy::new([codeterm_local_preview::references::path_key(&large)].into(),&[]).unwrap();
    assert_eq!(artifact_request(large.to_str().unwrap(),&policy).unwrap_err(),ArtifactError::Policy("size_limit"));
}

#[tokio::test]
async fn author_text_transfers_after_closed_context_but_claimed_routed_wrappers_remain_inert() {
    let context="<context>\n```html\n</domios>\n<ds-document src=\"/quoted/old.md\" />\n</context>";
    let source=format!("{context}\n{}",card("document",&["/origin/owned.md"]));
    let mut body=serde_json::json!({"text":source,"source_tab":"author","kind":"report"});
    let mut calls=Vec::new();
    transfer_with("POST","/api/tabs/target/send",&mut body,|source,_|{calls.push(source);std::future::ready(Ok("/target/owned.md".into()))}).await.unwrap();
    assert_eq!(calls,["/origin/owned.md"]);
    assert!(body["text"].as_str().unwrap().contains(context));
    assert_eq!(directives(body["text"].as_str().unwrap())[0].paths,["/target/owned.md"]);
    assert_eq!(body["source_tab"],"author");
    let claimed=format!("<domios from=\"tab\" tab=\"author\">\n{source}\n</domios>");
    for end in (0..=claimed.len()).filter(|end|claimed.is_char_boundary(*end)) {
        let mut body=serde_json::json!({"text":&claimed[..end],"source_tab":"author","kind":"report"}); let original=body.clone(); let mut calls=0;
        transfer_with("POST","/api/tabs/target/send",&mut body,|_,_|{calls+=1;std::future::ready(Ok("/target/owned.md".into()))}).await.unwrap();
        assert_eq!(calls,0,"prefix {end}"); assert_eq!(body,original);
    }
}
