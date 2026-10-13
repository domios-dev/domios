use super::*;

#[test]
fn prompt_identity_copies_opaque_request_and_distinguishes_successive_requests() {
    let mut prompt: PendingPrompt = serde_json::from_value(serde_json::json!({
        "question": "opaque", "options": [{"label":"label", "value":"native"}]
    }))
    .unwrap();
    let legacy = PromptIdentity::new(PromptKind::Provider, &prompt);
    assert!(serde_json::to_value(&legacy)
        .unwrap()
        .get("request_id")
        .is_none());
    assert!(serde_json::to_value(&prompt)
        .unwrap()
        .get("request_id")
        .is_none());
    prompt.request_id = Some("request-a".into());
    let first = PromptIdentity::new(PromptKind::Provider, &prompt);
    assert_eq!(first.request_id.as_deref(), Some("request-a"));
    assert_eq!(first.options, prompt.options);
    assert_eq!(
        serde_json::from_value::<PromptIdentity>(serde_json::to_value(&first).unwrap()).unwrap(),
        first
    );
    prompt.request_id = Some("request-b".into());
    assert_ne!(PromptIdentity::new(PromptKind::Provider, &prompt), first);
}
