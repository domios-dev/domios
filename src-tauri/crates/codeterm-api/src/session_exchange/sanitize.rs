use serde_json::{Map, Value};

use codeterm_service_core::session::store::LEAF_SESSION_ID_KEY;

const KEYS: &str = "version timestamp workspaces active_workspace_id teams id name tabs active_tab_id cwd provisioned_cwd protected sort_order task_keys next_sequence issued description split_tree type purpose icon utility_created_at close_restore shell_profile tab_id title title_level agent_type agent_type_locked agent_session_id agent_identity role role_profile team_id parent_tab_id spawned_by_tab_id report_target_tab_id report_target_mesh_peer ownership can_spawn depth chat_view auto_approve permission_auto_confirm audience agent_session_model agent_planned_model agent_requested_model agent_provider_resolved_model model_fallback model_identity_mismatch chatbackend_plugin_id chatbackend_session_id surface watcher pending_delivery orientation ratio children agent_runtime_state agent_runtime_config agent_runtime_questions session_binding_nonce reported_session_binding_nonce agent_state turn_resume boot_stage reasoning_effort restorable_launch context_epochs prior_session_bindings task_binding roleless_tracked managed lifecycle_epoch lifecycle_operation_id plan_id task_id attempt_id spawn_handle bound_at failed_provider structured_runtime_mode context_files context_skills worker_restart captured requested_model model requested_reasoning_effort resolved_reasoning_effort args agent_name identity_instructions permission_bypass_requested runtime_launch_options provider_id binding_nonce mode additional_directories network_access excluded_commands personality temp_dir settings systemPrompt effort state epoch reason active_turn_started_at_ms last_turn_duration_ms session_id start_epoch actual actual_source requested source manager_tab_id manager_name manager_closed lifecycle created_at workspace_id provider messages total seq content text thinking toolName toolInput toolResult toolUseId toolFlowId blocks data kind role status nextCursor threadId turnId items openItems index cursors turns stage questions objective goal format sanitized histories measurements snapshot prompt_bytes identity_bytes leaf_bytes history_bytes history_pages record_type page_size";
const IDS: &[&str] = &[
    "id",
    "tab_id",
    "workspace_id",
    "active_workspace_id",
    "active_tab_id",
    "team_id",
    "parent_tab_id",
    "spawned_by_tab_id",
    "report_target_tab_id",
    "manager_tab_id",
    "executor_tab_id",
    "orchestrator_tab_id",
    "reporter_tab_id",
    "agent_session_id",
    "session_id",
    "plan_id",
    "task_id",
    "attempt_id",
    "spawn_handle",
    "assigned_tab_id",
    "parent_task_id",
    "depends_on",
    "section_ids",
    "toolFlowId",
    "threadId",
    "turnId",
    "binding_nonce",
    "session_binding_nonce",
    "reported_session_binding_nonce",
];
const METADATA: &[&str] = &[
    "version",
    "agent_type",
    "provider",
    "provider_id",
    "chatbackend_plugin_id",
    "role",
    "role_profile",
    "purpose",
    "audience",
    "orientation",
    "shell_profile",
    "title_level",
    "model",
    "agent_session_model",
    "agent_requested_model",
    "agent_planned_model",
    "agent_provider_resolved_model",
    "requested_model",
    "effort",
    "mode",
    "lifecycle",
    "mimeType",
    "requested_reasoning_effort",
    "resolved_reasoning_effort",
    "actual",
    "actual_source",
    "planning_state",
    "priority",
    "paused_from_mode",
    "remote_dispatch_state",
    "taken_by_role",
    "author",
    "checkpoint_refusal",
    "refusal",
    "scope",
    "stage",
    "status",
    "placement",
    "execution_intent",
    "continuation",
    "interruption",
    "qa_state",
    "review_stage",
    "structured_runtime_mode",
    "boot_stage",
    "type",
    "kind",
    "state",
];

pub(super) fn filler(text: &str) -> String {
    text.chars()
        .map(|character| match character {
            '"' | '\\' => '\\',
            '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => '\n',
            '\u{0}'..='\u{1f}' => '\u{0}',
            _ => match character.len_utf8() {
                1 => 'x',
                2 => '¢',
                3 => '□',
                _ => '🟦',
            },
        })
        .collect()
}

fn opaque(key: &str) -> bool {
    matches!(
        key,
        "toolInput" | "toolResult" | "data" | "context_files" | "ownership"
    )
}

fn private_key(field: &str, result: &Map<String, Value>) -> Result<String, String> {
    let mut characters: Vec<char> = filler(field).chars().collect();
    let Some(index) = characters
        .iter()
        .rposition(|c| *c != '\\' && !c.is_control())
    else {
        let key = characters.into_iter().collect::<String>();
        return (!result.contains_key(&key))
            .then_some(key)
            .ok_or_else(|| "Cannot distinguish opaque keys at their original size".into());
    };
    let (start, count) = match characters[index].len_utf8() {
        1 => (u32::from(b'a'), 26),
        2 => (0x100, 256),
        3 => (0x2500, 1024),
        _ => (0x1f400, 1024),
    };
    for offset in 0..count {
        characters[index] = char::from_u32(start + offset).expect("Filler scalar range");
        let key = characters.iter().collect::<String>();
        if !result.contains_key(&key) {
            return Ok(key);
        }
    }
    Err("Opaque key inventory exceeds same-size filler capacity".into())
}

fn walk(value: &Value, key: &str, private: bool, schema: bool) -> Result<Value, String> {
    Ok(match value {
        Value::String(text) => {
            let preserve = !private
                && (IDS.contains(&key) || key == LEAF_SESSION_ID_KEY || METADATA.contains(&key));
            Value::String(if preserve { text.clone() } else { filler(text) })
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| walk(item, key, private, schema))
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(object) => {
            let mut result = Map::new();
            for (field, value) in object {
                let known = !private
                    && (schema
                        || KEYS
                            .split_whitespace()
                            .chain([LEAF_SESSION_ID_KEY])
                            .any(|key| key == field));
                let safe_field = if known {
                    field.clone()
                } else {
                    private_key(field, &result)?
                };
                if result.contains_key(&safe_field) {
                    return Err("Sanitization cannot preserve distinct opaque object keys at their original size".into());
                }
                result.insert(
                    safe_field,
                    walk(value, field, private || opaque(field) || !known, schema)?,
                );
            }
            Value::Object(result)
        }
        Value::Number(number) if private => {
            let raw = number.to_string();
            let masked: String = raw
                .chars()
                .map(|c| if c.is_ascii_digit() { '1' } else { c })
                .collect();
            let value: Value =
                serde_json::from_str(&masked).map_err(|_| "Cannot mask numeric secret")?;
            if serde_json::to_vec(&value).map_err(|e| e.to_string())?.len() != raw.len() {
                return Err("Cannot preserve numeric secret's serialized size".into());
            }
            value
        }
        _ => value.clone(),
    })
}

pub(super) fn sanitize(value: &Value) -> Result<Value, String> {
    let safe = walk(value, "", false, false)?;
    if serde_json::to_vec(value).map_err(|e| e.to_string())?.len()
        != serde_json::to_vec(&safe).map_err(|e| e.to_string())?.len()
    {
        return Err("Sanitization changed the serialized size".into());
    }
    Ok(safe)
}

#[cfg(test)]
mod tests;

pub(super) fn sanitize_plans(value: &Value) -> Result<Value, String> {
    walk(value, "", false, true)
}
