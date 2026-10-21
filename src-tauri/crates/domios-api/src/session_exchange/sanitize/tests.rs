use super::*;
use serde_json::json;

#[test]
fn filler_preserves_utf8_utf16_and_json_escape_sizes() {
    let original = "secreté中🙂\"\\\n\r\t\u{8}\u{c}\u{0}\u{1f}";
    let safe = filler(original);
    assert_ne!(original, safe);
    assert_eq!(original.len(), safe.len());
    assert_eq!(original.encode_utf16().count(), safe.encode_utf16().count());
    assert_eq!(
        serde_json::to_vec(original).unwrap().len(),
        serde_json::to_vec(&safe).unwrap().len()
    );
}

#[test]
fn keeps_binding_and_role_while_masking_prompts_and_secrets() {
    let raw = json!({"tab_id":"tab-bound","agent_type":"fixture-agent","role":"Worker",
        "task_binding":{"plan_id":"plan-bound","task_id":"task-bound","attempt_id":"attempt-bound"},
        "agent_identity":{"restorable_launch":{"identity_instructions":"private persona"},
        "agent_runtime_config":{"systemPrompt":"private prompt","api_key":"private token"}}});
    let safe = sanitize(&raw).unwrap();
    assert_eq!(safe["task_binding"], raw["task_binding"]);
    assert_eq!(safe["role"], raw["role"]);
    assert_eq!(safe["agent_type"], raw["agent_type"]);
    assert!(!safe.to_string().contains("private"));
    assert!(!safe.to_string().contains("api_key"));
}

#[test]
fn opaque_payload_cannot_leak_through_structural_field_names() {
    let raw = json!({"toolInput":{"id":"secret-id","model":"secret-model","token":12345}});
    let safe = sanitize(&raw).unwrap();
    assert!(!safe.to_string().contains("secret"));
    assert!(!safe.to_string().contains("12345"));
}

#[test]
fn opaque_keys_keep_inventory_and_encoded_size_without_content() {
    let raw = json!({"toolInput":{"aa":"one","bb":"two"}});
    let safe = sanitize(&raw).unwrap();
    assert_eq!(safe["toolInput"].as_object().unwrap().len(), 2);
    assert_eq!(safe.to_string().len(), raw.to_string().len());
    assert!(!safe.to_string().contains("aa"));
    assert!(!safe.to_string().contains("bb"));
}

#[test]
fn sanitized_plan_keeps_durable_attempt_state_and_deserializes() {
    let plan: codeterm_types::plan::Plan = serde_json::from_value(json!({
        "id":"plan-bound", "title":"private plan", "status":"active", "mode":"execution",
        "owner":{"tab_id":"owner-tab", "session_id":"owner-session", "title":"private owner"},
        "project_path":"/private/path", "created_at":1,"updated_at":2,
        "tasks":[{"id":"task-bound","name":"private task","type":"implementation", "status":"in_progress",
            "body":"private task body","assigned_tab_id":"worker-tab",
            "execution_attempts":[{"id":"attempt-bound","executor_tab_id":"worker-tab",
                "acknowledged_at":3,"closed_at":null,"retired_at":null}]}]
    })).unwrap();
    let raw = serde_json::to_value(plan).unwrap();
    let safe = sanitize_plans(&raw).unwrap();
    assert!(!safe.to_string().contains("private"));
    let restored: codeterm_types::plan::Plan = serde_json::from_value(safe).unwrap();
    assert_eq!(
        restored.tasks[0].assigned_tab_id.as_deref(),
        Some("worker-tab")
    );
    let attempt = &restored.tasks[0].execution_attempts[0];
    assert_eq!(attempt.id, "attempt-bound");
    assert_eq!(attempt.executor_tab_id, "worker-tab");
    assert_eq!(attempt.acknowledged_at, Some(3));
    assert_eq!(attempt.closed_at, None);
    assert_eq!(attempt.retired_at, None);
}
