use std::fmt;

use serde_json::Value;

pub(crate) const TASK_UPDATE_KEYS: &[&str] = &[
    "acceptance_criteria",
    "acceptance_criteria_replace",
    "body",
    "competition",
    "correctness",
    "dependency_task_ids",
    "depends_on",
    "executor_role",
    "external_refs",
    "group",
    "handle",
    "intent",
    "kind",
    "lane",
    "model",
    "name",
    "parent",
    "parent_task_id",
    "personality",
    "personality_provenance",
    "primary",
    "priority",
    "qa_plan",
    "role_guidance",
    "sections",
    "status",
    "status_label",
    "tags",
    "test_plan",
    "type",
    "why",
];

const SOCKET_CONTEXT_KEYS: &[&str] = &[
    "caller_tab_id",
    "owner_epoch",
    "plan_id",
    "session_binding_nonce",
    "session_id",
    "task_id",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TaskUpdateSeam {
    Rest,
    Socket,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TaskUpdateInputError {
    pub(crate) unknown_key: Option<String>,
    accepted_keys: Vec<&'static str>,
    message: Option<String>,
}

impl fmt::Display for TaskUpdateInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(message) = &self.message {
            return formatter.write_str(message);
        }
        if let Some(key) = &self.unknown_key {
            write!(formatter, "unknown task update key '{key}'; ")?;
        } else {
            write!(formatter, "task update must be a JSON object; ")?;
        }
        write!(
            formatter,
            "accepted keys: {}",
            self.accepted_keys.join(", ")
        )
    }
}

pub(crate) fn validate_task_update_fields(
    body: &Value,
    seam: TaskUpdateSeam,
) -> Result<(), TaskUpdateInputError> {
    let accepted_keys = accepted_keys(seam);
    let Some(fields) = body.as_object() else {
        return Err(TaskUpdateInputError {
            unknown_key: None,
            accepted_keys,
            message: None,
        });
    };
    let mut unknown_keys = fields
        .keys()
        .filter(|key| !accepted_keys.contains(&key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    unknown_keys.sort_unstable();
    if let Some(unknown_key) = unknown_keys.into_iter().next() {
        return Err(TaskUpdateInputError {
            unknown_key: Some(unknown_key),
            accepted_keys,
            message: None,
        });
    }
    if let Some(personality) = fields.get("personality") {
        let Some(personality) = personality.as_str() else {
            if !personality.is_null() {
                return Err(TaskUpdateInputError {
                    unknown_key: None,
                    accepted_keys,
                    message: Some("personality must be a one-word executor name or null".into()),
                });
            }
            return Ok(());
        };
        if let Err(message) = crate::plan::validate_executor_name(personality) {
            return Err(TaskUpdateInputError {
                unknown_key: None,
                accepted_keys,
                message: Some(message),
            });
        }
    }
    Ok(())
}

fn accepted_keys(seam: TaskUpdateSeam) -> Vec<&'static str> {
    let mut keys = TASK_UPDATE_KEYS.to_vec();
    if seam == TaskUpdateSeam::Socket {
        keys.extend_from_slice(SOCKET_CONTEXT_KEYS);
    }
    keys.sort_unstable();
    keys
}

#[cfg(test)]
pub(crate) fn valid_task_update_payloads(
    parent_task_id: &str,
    dependency_task_id: &str,
) -> Vec<(&'static str, Value)> {
    vec![
        (
            "acceptance_criteria",
            serde_json::json!({ "acceptance_criteria": ["Criterion"] }),
        ),
        (
            "acceptance_criteria_replace",
            serde_json::json!({ "acceptance_criteria_replace": true }),
        ),
        ("body", serde_json::json!({ "body": "Updated body" })),
        (
            "competition",
            serde_json::json!({ "competition": { "enabled": true, "attempts": 2 } }),
        ),
        (
            "correctness",
            serde_json::json!({ "correctness": "Correct result" }),
        ),
        (
            "dependency_task_ids",
            serde_json::json!({ "dependency_task_ids": [dependency_task_id] }),
        ),
        (
            "depends_on",
            serde_json::json!({ "depends_on": [dependency_task_id] }),
        ),
        (
            "executor_role",
            serde_json::json!({ "executor_role": "rust-backend-dev" }),
        ),
        (
            "external_refs",
            serde_json::json!({ "external_refs": ["rollacode/codeterm-releases#18"] }),
        ),
        ("group", serde_json::json!({ "group": "stream-a" })),
        ("handle", serde_json::json!({ "handle": "updated-handle" })),
        ("intent", serde_json::json!({ "intent": "Updated intent" })),
        ("kind", serde_json::json!({ "kind": "review" })),
        (
            "lane",
            serde_json::json!({ "lane": { "branch": "lane/update-test" } }),
        ),
        ("model", serde_json::json!({ "model": "test:model" })),
        ("name", serde_json::json!({ "name": "Updated name" })),
        ("parent", serde_json::json!({ "parent": parent_task_id })),
        (
            "parent_task_id",
            serde_json::json!({ "parent_task_id": parent_task_id }),
        ),
        ("personality", serde_json::json!({ "personality": "Dumas" })),
        (
            "personality_provenance",
            serde_json::json!({ "personality_provenance": "user_supplied" }),
        ),
        ("primary", serde_json::json!({ "primary": "area:test" })),
        ("priority", serde_json::json!({ "priority": "p2" })),
        ("qa_plan", serde_json::json!({ "qa_plan": "Run QA" })),
        (
            "role_guidance",
            serde_json::json!({ "role_guidance": "Review carefully" }),
        ),
        ("sections", serde_json::json!({ "sections": "" })),
        ("status", serde_json::json!({ "status": "pending" })),
        (
            "status_label",
            serde_json::json!({ "status_label": "ready" }),
        ),
        ("tags", serde_json::json!({ "tags": ["area:test"] })),
        (
            "test_plan",
            serde_json::json!({ "test_plan": "Run focused test" }),
        ),
        ("type", serde_json::json!({ "type": "review" })),
        ("why", serde_json::json!({ "why": "Routing reason" })),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;

    #[test]
    fn every_supported_update_key_is_accepted_at_both_seams() {
        let payloads = valid_task_update_payloads("task_parent", "task_dependency");
        let payload_keys = payloads.iter().map(|(key, _)| *key).collect::<Vec<_>>();
        assert_eq!(payload_keys, TASK_UPDATE_KEYS);
        for (key, body) in payloads {
            assert_eq!(body.as_object().unwrap().len(), 1, "payload for {key}");
            assert!(body.get(key).is_some(), "payload for {key}");
            assert!(
                validate_task_update_fields(&body, TaskUpdateSeam::Socket).is_ok(),
                "Socket rejected supported key {key}"
            );
            assert!(
                validate_task_update_fields(&body, TaskUpdateSeam::Rest).is_ok(),
                "REST rejected supported key {key}"
            );
        }
    }

    #[test]
    fn arbitrary_and_future_shaped_unknown_keys_are_actionable_at_both_seams() {
        for seam in [TaskUpdateSeam::Socket, TaskUpdateSeam::Rest] {
            for key in ["nonsense_field", "external_ref", "externalRefs"] {
                let body = Value::Object(Map::from_iter([(key.to_string(), Value::Bool(true))]));
                let error = validate_task_update_fields(&body, seam).unwrap_err();
                assert_eq!(error.unknown_key.as_deref(), Some(key));
                assert!(error.to_string().contains(key));
                assert!(error.to_string().contains("accepted keys:"));
                assert!(error.to_string().contains("external_refs"));
            }
        }
    }

    #[test]
    fn socket_context_keys_are_not_a_second_update_field_set() {
        for key in SOCKET_CONTEXT_KEYS {
            let body = Value::Object(Map::from_iter([(key.to_string(), Value::Null)]));
            assert!(validate_task_update_fields(&body, TaskUpdateSeam::Socket).is_ok());
            assert!(validate_task_update_fields(&body, TaskUpdateSeam::Rest).is_err());
        }
    }

    #[test]
    fn socket_task_update_accepts_trusted_session_identity_before_field_authorization() {
        let body = serde_json::json!({
            "task_id": "task",
            "caller_tab_id": "planner",
            "owner_epoch": 0,
            "session_id": "session",
            "session_binding_nonce": "nonce",
            "name": "Structural update",
        });
        assert!(validate_task_update_fields(&body, TaskUpdateSeam::Socket).is_ok());
        assert!(validate_task_update_fields(&body, TaskUpdateSeam::Rest).is_err());
    }

    #[test]
    fn tab_name_contract_update_seams_reject_invalid_executor_names() {
        for seam in [TaskUpdateSeam::Socket, TaskUpdateSeam::Rest] {
            for value in ["rust-backend-dev", "Ada Lovelace"] {
                let error =
                    validate_task_update_fields(&serde_json::json!({ "personality": value }), seam)
                        .unwrap_err();
                assert!(error.to_string().contains("one word"), "{value}: {error}");
            }
        }
    }
}
