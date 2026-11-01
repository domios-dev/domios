use super::*;
use crate::store::MemItem;

fn hit(id: &str) -> SearchHit {
    SearchHit {
        item: MemItem {
            author: Default::default(),
            id: id.into(),
            uid: "u".into(),
            kind: "fact".into(),
            scope: "global".into(),
            title: id.into(),
            summary: None,
            area: None,
            tags: vec![],
            body: id.into(),
            source: None,
            importance: 3.0,
            created_at: "".into(),
            updated_at: "".into(),
            accessed_at: None,
            access_count: 0,
            recurrence_count: 0,
        },
        score: 1.0,
    }
}

#[test]
fn noop_preserves_order_truncated() {
    let r = NoopReranker;
    let hits = vec![hit("a"), hit("b"), hit("c")];
    let out = r.rerank("q", hits, 2);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].item.id, "a");
}

#[test]
fn build_disabled_returns_noop() {
    let cfg = RerankConfig::default();
    let r = build(&cfg);
    let hits = vec![hit("a")];
    assert_eq!(r.rerank("q", hits, 5).len(), 1);
}

#[test]
fn build_haiku_without_key_falls_back() {
    let cfg = RerankConfig {
        enabled: true,
        provider: "haiku".into(),
        api_key_env: Some("DEFINITELY_NOT_SET_KEY_12345".into()),
        model: None,
    };
    let r = build(&cfg);
    let hits = vec![hit("a"), hit("b")];
    let out = r.rerank("q", hits, 5);
    assert_eq!(out.len(), 2);
}

#[test]
fn which_on_path_finds_known_binary() {
    let _env = codeterm_util::test_env::EnvGuard::lock();
    assert!(
        which_on_path("rustc").is_some(),
        "expected rustc to be resolvable on PATH (running test implies it is)"
    );
    assert!(which_on_path("definitely_not_a_real_binary_xyz").is_none());
}

#[test]
fn build_claude_cli_falls_back_when_missing() {
    let mut env = codeterm_util::test_env::EnvGuard::lock();
    env.set("PATH", "");
    let cfg = RerankConfig {
        enabled: true,
        provider: "claude-cli".into(),
        api_key_env: None,
        model: None,
    };
    let r = build(&cfg);
    let hits = vec![hit("a")];
    assert_eq!(r.rerank("q", hits, 5).len(), 1);
}

#[test]
fn auto_without_a_configured_key_preserves_input_order() {
    let cfg = RerankConfig {
        enabled: true,
        provider: "".into(),
        api_key_env: Some("DEFINITELY_UNSET_KEY".into()),
        model: None,
    };
    let hits = vec![hit("a"), hit("b")];
    let result = build(&cfg).rerank("q", hits, 1);
    assert_eq!(
        result
            .iter()
            .map(|hit| hit.item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["a"]
    );
}

#[test]
fn build_codex_without_key_falls_back() {
    let cfg = RerankConfig {
        enabled: true,
        provider: "codex".into(),
        api_key_env: Some("DEFINITELY_NOT_SET_KEY_67890".into()),
        model: None,
    };
    let r = build(&cfg);
    let hits = vec![hit("a")];
    assert_eq!(r.rerank("q", hits, 5).len(), 1);
}

#[test]
fn unsupported_provider_preserves_input_order_without_loading_weights() {
    let cfg = RerankConfig {
        enabled: true,
        provider: "local".into(),
        api_key_env: None,
        model: Some("jina-v1-turbo-en".into()),
    };
    let result = build(&cfg).rerank("q", vec![hit("a"), hit("b")], 1);
    assert_eq!(result[0].item.id, "a");
}

#[test]
fn parse_id_array_works_on_pure_json() {
    let r = parse_id_array(r#"["a", "b", "c"]"#).unwrap();
    assert_eq!(r, vec!["a", "b", "c"]);
}

#[test]
fn parse_id_array_extracts_from_prose() {
    let r = parse_id_array("Sure, here are the ranked ids: [\"x\", \"y\"] in best order.").unwrap();
    assert_eq!(r, vec!["x", "y"]);
}
