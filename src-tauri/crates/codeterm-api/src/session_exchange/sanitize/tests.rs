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
    use codeterm_types::plan::{
        CuratorAsked, CuratorFailure, CuratorOutcome, CuratorRound, CuratorSuspension,
    };

    let mut plan: codeterm_types::plan::Plan = serde_json::from_value(json!({
        "id":"plan-bound", "title":"private plan", "status":"active", "mode":"execution",
        "owner":{"tab_id":"owner-tab", "session_id":"owner-session", "title":"private owner"},
        "project_path":"/private/path", "created_at":1,"updated_at":2,
        "tasks":[{"id":"task-bound","name":"private task","type":"implementation", "status":"in_progress",
            "body":"private task body","assigned_tab_id":"worker-tab",
            "execution_attempts":[{"id":"attempt-bound","executor_tab_id":"worker-tab",
                "acknowledged_at":3,"closed_at":null,"retired_at":null}]}]
    })).unwrap();
    let asked_variants = [
        CuratorAsked::Unrecorded,
        CuratorAsked::None,
        CuratorAsked::Answered,
        CuratorAsked::Skip,
        CuratorAsked::Timeout,
    ];
    let outcomes = [
        CuratorOutcome::Quiet,
        CuratorOutcome::Nudged,
        CuratorOutcome::Done,
        CuratorOutcome::Partial,
        CuratorOutcome::Failed,
        CuratorOutcome::Fallback,
        CuratorOutcome::ModeChanged,
    ];
    let private = "private round é中🙂\"\\\n\r\t\u{0}";
    plan.control_loop.fallback_reason = Some(CuratorFailure::MirrorUnavailable);
    plan.control_loop.suspended_reason = Some(CuratorSuspension::WaitingOnHuman);
    plan.control_loop.rounds = outcomes
        .into_iter()
        .enumerate()
        .map(|(index, outcome)| CuratorRound {
            number: index as u64 + 1,
            asked: asked_variants[index % asked_variants.len()],
            outcome,
            tab_id: Some("curator-tab".into()),
            run_id: Some(private.into()),
            move_line: private.into(),
            retro: vec![private.into()],
            learn: vec![private.into()],
            next_checks: vec![private.into()],
            ready: Some(private.into()),
            summary: private.into(),
            reason: private.into(),
            save_id: Some(private.into()),
            digest_hash: private.into(),
            section_sizes: [("summary".into(), 42)].into(),
            ..Default::default()
        })
        .collect();

    for asked in asked_variants {
        let mut last_round = plan.control_loop.rounds[0].clone();
        last_round.asked = asked;
        plan.control_loop.last_round = Some(last_round);
        let raw = serde_json::to_value(&plan).unwrap();
        for document in [raw.clone(), json!([raw.clone()])] {
            let safe = sanitize_plans(&document).unwrap();
            assert_same_inventory_and_sizes(&document, &safe);
            assert!(!safe.to_string().contains("private"));
            let restored = if safe.is_array() {
                serde_json::from_value::<Vec<codeterm_types::plan::Plan>>(safe)
                    .unwrap()
                    .remove(0)
            } else {
                serde_json::from_value::<codeterm_types::plan::Plan>(safe).unwrap()
            };
            assert_eq!(
                restored.control_loop.last_round.as_ref().unwrap().asked,
                asked
            );
            assert_eq!(restored.control_loop.rounds.len(), 7);
            for (original, restored) in plan
                .control_loop
                .rounds
                .iter()
                .zip(&restored.control_loop.rounds)
            {
                assert_eq!(restored.asked, original.asked);
                assert_eq!(restored.outcome, original.outcome);
                assert_eq!(restored.summary, filler(private));
                assert_eq!(restored.tab_id.as_deref(), Some("curator-tab"));
            }
            assert_eq!(
                restored.control_loop.fallback_reason,
                Some(CuratorFailure::MirrorUnavailable)
            );
            assert_eq!(
                restored.control_loop.suspended_reason,
                Some(CuratorSuspension::WaitingOnHuman)
            );
            assert_durable_attempt(&restored);
        }
    }
}

fn assert_durable_attempt(restored: &codeterm_types::plan::Plan) {
    assert_eq!(restored.id, "plan-bound");
    assert_eq!(restored.owner.tab_id.as_deref(), Some("owner-tab"));
    assert_eq!(restored.owner.session_id.as_deref(), Some("owner-session"));
    assert_eq!(restored.tasks[0].id, "task-bound");
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

fn assert_same_inventory_and_sizes(raw: &Value, safe: &Value) {
    assert_eq!(
        serde_json::to_vec(raw).unwrap().len(),
        serde_json::to_vec(safe).unwrap().len()
    );
    match (raw, safe) {
        (Value::Object(raw), Value::Object(safe)) => {
            assert_eq!(
                raw.keys().collect::<Vec<_>>(),
                safe.keys().collect::<Vec<_>>()
            );
            for (key, value) in raw {
                assert_same_inventory_and_sizes(value, &safe[key]);
            }
        }
        (Value::Array(raw), Value::Array(safe)) => {
            assert_eq!(raw.len(), safe.len());
            for (raw, safe) in raw.iter().zip(safe) {
                assert_same_inventory_and_sizes(raw, safe);
            }
        }
        (Value::String(raw), Value::String(safe)) => {
            assert_eq!(raw.len(), safe.len());
            assert_eq!(raw.encode_utf16().count(), safe.encode_utf16().count());
        }
        _ => assert_eq!(raw, safe),
    }
}

#[test]
fn curator_enum_preservation_requires_structural_path_and_valid_variant() {
    for text in ["private asked é中🙂\"\\\n", "none private", "evil"] {
        let raw = json!({"control_loop":{"last_round":{"asked":text,"outcome":text},
            "rounds":[{"asked":text,"outcome":text}],
            "fallback_reason":text,"suspended_reason":text}});
        let safe = sanitize_plans(&raw).unwrap();
        assert_same_inventory_and_sizes(&raw, &safe);
        assert_eq!(safe["control_loop"]["last_round"]["asked"], filler(text));
        assert_eq!(safe["control_loop"]["rounds"][0]["asked"], filler(text));
        assert_eq!(safe["control_loop"]["last_round"]["outcome"], filler(text));
        assert_eq!(safe["control_loop"]["fallback_reason"], filler(text));
        assert_eq!(safe["control_loop"]["suspended_reason"], filler(text));
        assert!(!safe.to_string().contains(text));
        assert!(serde_json::from_value::<codeterm_types::plan::ControlLoop>(
            safe["control_loop"].clone()
        )
        .is_err());
    }
    let raw = json!({"asked":"answered","outcome":"quiet",
        "control_loop":{"asked":"skip","summary":{"asked":"timeout"},
            "last_round":{"summary":{"asked":"none"},"asked":["answered"]}},
        "content":{"control_loop":{"last_round":{"asked":"none","outcome":"quiet"}}}});
    let safe = sanitize_plans(&raw).unwrap();
    assert_same_inventory_and_sizes(&raw, &safe);
    for text in ["answered", "quiet", "skip", "timeout", "none"] {
        assert!(!safe.to_string().contains(text));
    }
    let safe = sanitize(&json!({"control_loop":{"last_round":{"asked":"none"}}})).unwrap();
    assert!(!safe.to_string().contains("none"));
}

#[test]
fn opaque_curator_lookalikes_keep_masked_inventory_and_size() {
    for key in [
        "toolInput",
        "toolResult",
        "data",
        "context_files",
        "ownership",
    ] {
        let payload = json!({"asked":"answered","outcome":"quiet","id":"private-id",
            "control_loop":{"last_round":{"asked":"none","outcome":"fallback"},
                "rounds":[{"asked":"timeout"}],"fallback_reason":"mirror_unavailable"}});
        let raw = json!({(key):payload});
        for safe in [sanitize(&raw).unwrap(), sanitize_plans(&raw).unwrap()] {
            assert_eq!(safe.to_string().len(), raw.to_string().len());
            let object = safe[key].as_object().unwrap();
            assert_eq!(object.len(), 4);
            for text in [
                "asked",
                "outcome",
                "answered",
                "quiet",
                "private",
                "control_loop",
                "none",
                "fallback",
                "timeout",
                "mirror_unavailable",
            ] {
                assert!(!safe[key].to_string().contains(text));
            }
        }
    }
}
