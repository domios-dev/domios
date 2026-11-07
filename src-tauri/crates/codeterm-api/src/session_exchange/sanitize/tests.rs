use super::*;
use serde_json::json;

#[test]
fn typed_desk_plans_preserve_enums_and_mask_all_free_text() {
    use codeterm_types::plan::Plan;

    for planning_state in ["drafting", "structure_ready", "approved"] {
        let statuses = [
            "pending",
            "in_progress",
            "in_review",
            "blocked",
            "paused",
            "done",
            "cancelled",
        ];
        let tasks: Vec<Value> = statuses.into_iter().map(|status| json!({
            "id":format!("task-{status}"), "name":"private task", "type":"private kind",
            "status":status, "priority":"p0", "review_stage":"qa", "qa_state":"running",
            "execution_intent":"repository_mutation", "body":"private body", "executor_role":"private role",
            "taken_by_role":"Worker", "model":"private model",
            "execution_attempts":[{"id":"attempt-bound", "executor_tab_id":"worker-tab",
                "role":"Worker", "placement":"shared", "remote_dispatch_state":"acknowledged",
                "continuation":{"epoch":1, "state":"continued"},
                "interruption":{"owner_epoch":1, "reason":"private interruption"},
                "report":{"id":"report-bound", "status":"in_review", "message":"private report"}},
                {"id":"attempt-unrecognized", "executor_tab_id":"worker-tab", "role":"private role",
                    "continuation":{"epoch":1, "state":"private state"},
                    "report":{"id":"report-unrecognized", "status":"private status"}}]
        })).collect();
        let mut plan: Plan = serde_json::from_value(json!({
            "id":"plan-bound", "title":"private plan", "status":"active", "mode":"paused",
            "paused_from_mode":"execution", "planning_state":planning_state,
            "owner":{"tab_id":"owner-tab", "session_id":"owner-session", "title":"private owner"},
            "project_path":"private path", "created_at":1, "updated_at":2, "tasks":tasks,
            "planner":{"kind":"waived", "note":"private waiver"},
            "planning_log":[{"from":"drafting", "to":"approved", "by":{"kind":"planner", "tab_id":"planner-tab"}, "note":"private note", "at":3}],
            "planner_requests":[{"request_id":"request-bound", "payload":"private request", "state":"in_progress", "created_at":3}],
            "planner_inbox":[{"request_id":"request-bound", "phase":"provider_accepted", "transport":"pty_agent", "last_error":"private error", "updated_at":4}],
            "pause_operations":[{"id":"pause-bound", "paused_at":5, "paused_from_mode":"execution",
                "reason":"private pause", "stage":"incomplete", "effects":["team_schedules"],
                "agents":[{"tab_id":"worker-tab", "name":"private agent", "role":"worker",
                    "pause_notice":{"state":"failed", "error":"private failure"},
                    "resume_notice":{"state":"delivered"}}]}],
            "elements":[{"id":"element-bound", "kind":"private kind", "content":{"status":"private nested status", "role":"Worker", "id":"private nested id"}}]
        })).unwrap();
        let directory = tempfile::tempdir().unwrap();
        plan.project_path = directory
            .path()
            .join("private-project")
            .display()
            .to_string();
        plan.canonical_project_path = plan.project_path.clone();
        let store = codeterm_service_core::service::plan::PlanStore::new_for_test(directory.path());
        store.save(&mut plan, 2).unwrap();
        codeterm_service_core::service::plan::load_audit::start(&store);
        let raw = serde_json::to_value(&plan).unwrap();
        let safe = sanitize_plan(&plan).unwrap();
        assert_eq!(
            store
                .export_all_plan_documents_with(&SanitizedPlanRows)
                .unwrap(),
            vec![safe.clone()]
        );
        assert!(store.plan_loads_for_test().is_empty());
        assert_eq!(
            serde_json::to_vec(&raw).unwrap().len(),
            serde_json::to_vec(&safe).unwrap().len()
        );
        assert!(!safe.to_string().contains("private"));
        for pointer in [
            "/title",
            "/owner/title",
            "/project_path",
            "/planner/note",
            "/planning_log/0/note",
            "/planner_requests/0/payload",
            "/planner_inbox/0/last_error",
            "/pause_operations/0/reason",
            "/pause_operations/0/agents/0/name",
            "/pause_operations/0/agents/0/pause_notice/error",
            "/elements/0/kind",
        ] {
            assert_masked_text(&raw, &safe, pointer);
        }
        assert!(!safe["elements"].to_string().contains("Worker"));
        let restored = serde_json::from_value::<Vec<Plan>>(json!([safe.clone()]))
            .unwrap()
            .remove(0);
        assert_eq!(restored.planning_state, plan.planning_state);
        for pointer in [
            "/status",
            "/mode",
            "/paused_from_mode",
            "/planner/kind",
            "/planning_log/0/from",
            "/planning_log/0/to",
            "/planning_log/0/by/kind",
            "/planner_requests/0/state",
            "/planner_inbox/0/phase",
            "/planner_inbox/0/transport",
            "/pause_operations/0/stage",
            "/pause_operations/0/effects",
            "/pause_operations/0/agents/0/role",
            "/pause_operations/0/agents/0/pause_notice/state",
            "/pause_operations/0/agents/0/resume_notice/state",
        ] {
            assert_eq!(safe.pointer(pointer), raw.pointer(pointer), "{pointer}");
        }
        for (index, task) in restored.tasks.iter().enumerate() {
            for field in [
                "name",
                "type",
                "body",
                "executor_role",
                "model",
                "execution_attempts/0/interruption/reason",
                "execution_attempts/0/report/message",
                "execution_attempts/1/role",
                "execution_attempts/1/continuation/state",
                "execution_attempts/1/report/status",
            ] {
                assert_masked_text(&raw, &safe, &format!("/tasks/{index}/{field}"));
            }
            assert_eq!(task.status, plan.tasks[index].status);
            assert_eq!(task.priority, plan.tasks[index].priority);
            assert_eq!(task.execution_intent, plan.tasks[index].execution_intent);
            assert_eq!(task.review_stage, plan.tasks[index].review_stage);
            assert_eq!(task.qa_state, plan.tasks[index].qa_state);
            assert_eq!(task.kind, filler("private kind"));
            let attempt = &safe["tasks"][index]["execution_attempts"][0];
            assert_eq!(attempt["role"], "Worker");
            assert_eq!(attempt["placement"], "shared");
            assert_eq!(attempt["remote_dispatch_state"], "acknowledged");
            assert_eq!(attempt["continuation"]["state"], "continued");
            assert_eq!(attempt["report"]["status"], "in_review");
            assert_eq!(attempt["id"], "attempt-bound");
            assert_eq!(
                safe["tasks"][index]["execution_attempts"][1]["role"],
                filler("private role")
            );
            let private_attempt = &safe["tasks"][index]["execution_attempts"][1];
            assert_eq!(private_attempt["id"], "attempt-unrecognized");
            assert_eq!(private_attempt["report"]["id"], "report-unrecognized");
            assert_eq!(
                private_attempt["continuation"]["state"],
                filler("private state")
            );
            assert_eq!(
                private_attempt["report"]["status"],
                filler("private status")
            );
        }
        let mut invalid = safe;
        invalid["planning_state"] = json!("xxxxxxxx");
        assert!(serde_json::from_value::<Plan>(invalid).is_err());
    }
}

fn assert_masked_text(raw: &Value, safe: &Value, pointer: &str) {
    let text = raw.pointer(pointer).unwrap().as_str().unwrap();
    assert!(
        text.contains("private"),
        "Missing fixture sentinel at {pointer}"
    );
    assert_eq!(
        safe.pointer(pointer),
        Some(&json!(filler(text))),
        "{pointer}"
    );
}

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
fn opaque_keys_use_all_positions_with_mixed_widths_and_escapes() {
    use serde::Serialize;

    for keys in [
        (0..1000)
            .map(|index| format!("secret-{index:04}"))
            .collect::<Vec<_>>(),
        (0..1100)
            .map(|index| {
                format!(
                    "{}é中🙂\"\\\n\u{0}",
                    char::from_u32(0x4000 + index).unwrap()
                )
            })
            .collect(),
        (0..1100)
            .map(|index| format!("é{}", char::from_u32(0x4000 + index).unwrap()))
            .collect(),
        (0..1100)
            .map(|index| format!("🙂{}", char::from_u32(0x1f400 + index).unwrap()))
            .collect(),
    ] {
        let raw: Map<String, Value> = keys
            .iter()
            .map(|key| (key.clone(), json!("private value")))
            .collect();
        let document = json!({"toolInput":raw});
        let safe = sanitize(&document).unwrap();
        assert_eq!(safe, sanitize(&document).unwrap());
        for masked in [
            safe["toolInput"].clone(),
            raw.serialize(typed::Sanitizer::default()).unwrap(),
        ] {
            let object = masked.as_object().unwrap();
            assert_eq!(object.len(), keys.len());
            assert_eq!(
                serde_json::to_vec(&raw).unwrap().len(),
                serde_json::to_vec(&masked).unwrap().len()
            );
            assert!(!masked.to_string().contains("private"));
            assert!(!masked.to_string().contains("secret"));
            for key in object.keys() {
                assert_eq!(key.len(), keys[0].len());
                assert_eq!(key.encode_utf16().count(), keys[0].encode_utf16().count());
                assert_eq!(
                    key.chars().map(char::len_utf8).collect::<Vec<_>>(),
                    keys[0].chars().map(char::len_utf8).collect::<Vec<_>>()
                );
            }
        }
    }
}

#[test]
fn opaque_keys_do_not_collide_with_preserved_fields() {
    let mut raw = Map::new();
    raw.insert("id".into(), json!("bound-id"));
    for index in 0..600 {
        let key = format!(
            "{}{}",
            char::from(b'A' + (index / 26) as u8),
            char::from(b'A' + (index % 26) as u8)
        );
        raw.insert(key, json!("private"));
    }
    let safe = sanitize(&Value::Object(raw.clone())).unwrap();
    assert_eq!(safe["id"], "bound-id");
    assert_eq!(safe.as_object().unwrap().len(), raw.len());
    assert_eq!(
        serde_json::to_vec(&raw).unwrap().len(),
        serde_json::to_vec(&safe).unwrap().len()
    );
    assert!(!safe.to_string().contains("private"));
}

#[test]
fn opaque_keys_refuse_only_after_all_filler_positions_are_exhausted() {
    for (field, capacity) in [("!", 26), ("!!", 26 * 26), ("", 1), ("\"\n", 1)] {
        let mut used = Map::new();
        for _ in 0..capacity {
            let key = private_key(field, &used).unwrap();
            assert_eq!(key.len(), field.len());
            assert_eq!(
                serde_json::to_vec(&key).unwrap().len(),
                serde_json::to_vec(field).unwrap().len()
            );
            assert!(used.insert(key, Value::Null).is_none());
        }
        assert!(private_key(field, &used).is_err());
    }
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
    let rounds: Vec<CuratorRound> = outcomes
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

    for round in rounds {
        let (asked, outcome) = (round.asked, round.outcome);
        plan.control_loop.last_round = Some(round);
        let raw = serde_json::to_value(&plan).unwrap();
        for document in [raw.clone(), json!([raw.clone()])] {
            let safe_plan = sanitize_plan(&plan).unwrap();
            let safe = if document.is_array() {
                json!([safe_plan])
            } else {
                safe_plan
            };
            assert_eq!(
                serde_json::to_vec(&document).unwrap().len(),
                serde_json::to_vec(&safe).unwrap().len()
            );
            assert!(!safe.to_string().contains("private"));
            let restored = if safe.is_array() {
                serde_json::from_value::<Vec<codeterm_types::plan::Plan>>(safe)
                    .unwrap()
                    .remove(0)
            } else {
                serde_json::from_value::<codeterm_types::plan::Plan>(safe).unwrap()
            };
            let restored_round = restored.control_loop.last_round.as_ref().unwrap();
            assert_eq!(restored_round.asked, asked);
            assert_eq!(restored_round.outcome, outcome);
            assert_eq!(restored_round.summary, filler(private));
            assert_eq!(restored_round.tab_id.as_deref(), Some("curator-tab"));
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
