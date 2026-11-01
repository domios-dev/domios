use super::{search_error, secret_socket_rejection, SearchError};

#[test]
fn precompact_purge_rpc_refuses_unknown_fields_and_preserves_partial_report() {
    let dir = tempfile::tempdir().unwrap();
    let mem = codeterm_service_core::mem::MemManager::open_offline(dir.path()).unwrap();
    let invalid = super::execute_precompact_purge(
        Some(serde_json::json!({"id":"x","unexpected":true})),
        &mem,
    )
    .unwrap_err();
    assert_eq!(invalid.code, -32602);
    let id = mem
        .save(codeterm_service_core::mem::store::NewMemItem {
            kind: codeterm_service_core::mem::store::MemKind::Conversation,
            scope: codeterm_service_core::mem::store::MemScope::Global,
            title: "flush:0123abcd pre-compact".into(),
            body: "fixture".repeat(1000),
            summary: None,
            area: None,
            tags: vec![],
            source: None,
            importance: 1.0,
            source_path: None,
            pinned: false,
        })
        .unwrap();
    mem.delete(&id).unwrap();
    let blob = std::fs::read_dir(mem.blobs_dir())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(blob, "changed").unwrap();
    let preview =
        super::execute_precompact_purge(Some(serde_json::json!({"id":id})), &mem).unwrap();
    assert_eq!(preview["dry_run"], true);
    let error = super::execute_precompact_purge(
        Some(serde_json::json!({"id":id,"apply":true,"verified_deleted":true})),
        &mem,
    )
    .unwrap_err();
    assert_eq!(error.code, -32000);
    let data = error.data.unwrap();
    assert_eq!(data["logical_purge_complete"], true);
    assert_eq!(data["blob_cleanup_complete"], false);
    assert_eq!(data["errors"].as_array().unwrap().len(), 1);
}

#[test]
fn secret_methods_are_explicitly_rejected_over_socket() {
    for method in [
        "mem.secret.save",
        "mem.secret.get",
        "mem.secret.list",
        "mem.secret.delete",
        "mem.secret.status",
        "mem.secret.unlock",
    ] {
        let err = secret_socket_rejection(method)
            .unwrap_or_else(|| panic!("{method} must be rejected over socket"));
        assert_eq!(err.code, -32004);
        assert!(err.message.contains("authenticated-HTTP-only"));
        assert!(err.message.contains("/api/secrets/"));
    }
}

#[test]
fn non_secret_methods_are_not_rejected() {
    for method in ["mem.get", "mem.save", "mem.graph.create", "mem.stats"] {
        assert!(
            secret_socket_rejection(method).is_none(),
            "{method} should not hit the secret rejection"
        );
    }
}

#[test]
fn supporter_socket_refuses_token_methods() {
    let secret = secret_socket_rejection("mem.secret.get").unwrap();
    for method in [
        "mem.supporter.token.set",
        "mem.supporter.token.clear",
        "mem.supporter.token.status",
    ] {
        let err = secret_socket_rejection(method)
            .unwrap_or_else(|| panic!("{method} must be rejected over socket"));
        assert_eq!(err.code, secret.code);
        assert!(err.message.contains("/api/supporter/token"));
    }
    assert!(secret_socket_rejection("mem.search").is_none());
}

#[test]
fn decision_socket_maps_exclusive_rerank_to_invalid_params() {
    let err = search_error(SearchError::InvalidParams("x".into()));
    assert_eq!(err.code, -32602);
    assert_eq!(err.message, "x");
    assert_eq!(search_error(SearchError::Internal("y".into())).code, -32000);
}
