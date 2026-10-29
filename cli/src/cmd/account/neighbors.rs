use crate::args::account::{NeighborAction, NeighborLimit};
use crate::cmd::print::{fail, fail_http, print_human};
use serde_json::{json, Value};
use zeroize::Zeroizing;

fn authored(path: &str) -> Zeroizing<String> {
    Zeroizing::new(
        crate::cmd::payload_source::read_from_pad(path).unwrap_or_else(|error| fail(1, error)),
    )
}

pub(super) fn handle(json_mode: bool, action: NeighborAction) {
    let listing = matches!(action, NeighborAction::List);
    let (path, command) = request(action);
    let client =
        crate::build_local_api().unwrap_or_else(|_| fail(1, "Local Neighbor service unavailable"));
    let response = client
        .post(path)
        .json(&command)
        .send()
        .unwrap_or_else(|_| fail(1, "Local Neighbor request failed"));
    let status = response.status();
    let body = response
        .text()
        .unwrap_or_else(|_| fail(1, "Invalid Neighbor response"));
    if !status.is_success() {
        fail_http(1, status.as_u16(), &body);
    }
    let result: Value =
        serde_json::from_str(&body).unwrap_or_else(|_| fail(1, "Invalid Neighbor response"));
    if json_mode {
        println!("{result}");
    } else if listing {
        println!("{}", render_list(&result));
    } else {
        print_human(&result);
    }
}

fn limit_value(limit: NeighborLimit) -> Value {
    match limit {
        NeighborLimit::Default => Value::Null,
        NeighborLimit::Value(value) => json!(value),
    }
}

fn request(action: NeighborAction) -> (&'static str, Value) {
    let command = match action {
        NeighborAction::Flag {
            message_id,
            friendship,
            category,
            reason,
        } => flag_command(message_id, friendship, category, reason)
            .unwrap_or_else(|error| fail(1, error)),
        NeighborAction::List => return ("/api/account/neighbors", json!({"operation":"list"})),
        NeighborAction::SetLimit {
            friendship_id,
            per_hour,
        } => {
            return (
                "/api/account/neighbors",
                json!({"operation":"set_limit","friendship_id":friendship_id,"per_hour":per_hour}),
            )
        }
        NeighborAction::SetDailyLimit {
            friendship_id,
            daily_limit,
        } => json!({
            "operation":"set_daily_limit","friendship_id":friendship_id,"daily_limit":limit_value(daily_limit),
        }),
        NeighborAction::SetFileDailyLimit {
            friendship_id,
            file_daily_limit_bytes,
        } => {
            return (
                "/api/account/neighbors/files",
                json!({"operation":"set_daily_limit","friendship_id":friendship_id,"file_daily_limit_bytes":limit_value(file_daily_limit_bytes)}),
            )
        }
        NeighborAction::ReceiveHere {
            friendship_id,
            expected_receiver_revision,
            takeover_confirmed,
        } => {
            json!({"operation":"set_friend_receiver","friendship_id":friendship_id,"receiver_operation":"set","expected_receiver_revision":expected_receiver_revision,"takeover_confirmed":takeover_confirmed})
        }
        NeighborAction::Status => json!({"operation":"status"}),
        NeighborAction::Enroll {
            confirm_takeover,
            expected_receiver_revision,
        } => {
            json!({"operation":"enroll","takeover_confirmed":confirm_takeover,"expected_receiver_revision":expected_receiver_revision})
        }
        NeighborAction::Rotate => json!({"operation":"rotate"}),
        NeighborAction::Revoke => json!({"operation":"revoke"}),
        NeighborAction::OwnKeys => json!({"operation":"own_keys"}),
        NeighborAction::Bind { friendship_id, tab } => {
            json!({"operation":"bind","friendship_id":friendship_id,"gateway_tab":tab})
        }
        NeighborAction::Permission { friendship_id } => {
            json!({"operation":"permission","friendship_id":friendship_id})
        }
        NeighborAction::Grant {
            friendship_id,
            fields,
        } => {
            let scope = fields.from_pad.as_deref().map(authored);
            grant_command(friendship_id, fields, scope.as_deref().map(String::as_str))
        }
        NeighborAction::Deny { friendship_id } => {
            json!({"operation":"set_permission","friendship_id":friendship_id,"permission":{"allowed":false}})
        }
        NeighborAction::Send {
            friendship_id,
            from_pad,
            kind,
            in_reply_to,
        } => {
            let body = authored(&from_pad);
            json!({"operation":"send","friendship_id":friendship_id,"kind":kind,"body":body.as_str(),"in_reply_to":in_reply_to})
        }
        NeighborAction::DeliveryStatus { message_id } => {
            json!({"operation":"delivery_status","message_id":message_id})
        }
    };
    ("/api/account/neighbors/runtime", command)
}

fn flag_command(
    message_id: String,
    friendship: String,
    category: String,
    reason: String,
) -> Result<Value, String> {
    if reason.trim().is_empty()
        || reason.chars().count() > 280
        || reason.chars().any(char::is_control)
    {
        return Err("Flag reason must be a short summary of 1 through 280 characters without message text or secrets".into());
    }
    Ok(
        json!({"operation":"flag_message","message_id":message_id,"friendship_id":friendship,"category":category,"reason":reason}),
    )
}

fn render_list(result: &Value) -> String {
    let mut lines = vec![];
    for (key, label, id_key) in [
        ("friends", "Friendships", "friendship_id"),
        ("incoming", "Incoming requests", "request_id"),
        ("outgoing", "Outgoing requests", "id"),
    ] {
        lines.push(format!("{label}:"));
        let Some(rows) = result[key].as_array() else {
            lines.push("  unavailable from backend".into());
            continue;
        };
        if rows.is_empty() {
            lines.push("  (none)".into());
        }
        for row in rows {
            let text = |key: &str| row[key].as_str().unwrap_or("");
            let name = ["displayName", "typed_email", "email", "uid"]
                .into_iter()
                .map(text)
                .find(|value| !value.is_empty())
                .unwrap_or("unknown");
            let id = text(id_key);
            if key == "friends" {
                let permission = &row["permission"];
                let scope = if permission["allowed"] == true {
                    permission["scope"].as_str().unwrap_or("")
                } else {
                    "denied"
                };
                lines.push(format!(
                    "  {id}\t{name}\t{}\tscope: {scope}\ttab requests: {}",
                    if row["receiving"] == false {
                        "not receiving right now"
                    } else {
                        text("status")
                    },
                    permission["allow_tab_requests"] == true
                ));
            } else {
                lines.push(format!(
                    "  {id}\t{name}\tpending\tcreated_at: {}",
                    row["created_at"]
                ));
            }
        }
    }
    lines.join("\n")
}

fn grant_command(
    friendship_id: String,
    fields: crate::args::account::NeighborGrant,
    scope: Option<&str>,
) -> Value {
    let mut permission = serde_json::Map::new();
    if let Some(scope) = scope {
        permission.insert("scope".into(), json!(scope));
        permission.insert("allowed".into(), json!(true));
    }
    for (field, value) in [
        ("allowed", fields.allowed),
        ("allow_memory", fields.allow_memory),
        ("allow_files", fields.allow_files),
        ("allow_tab_requests", fields.allow_tab_requests),
        (
            "allow_interactive_content",
            fields.allow_interactive_content,
        ),
    ] {
        if let Some(value) = value {
            permission.insert(field.into(), json!(value));
        }
    }
    if let Some(value) = fields.memory_areas {
        permission.insert("memory_areas".into(), json!(value.0));
    }
    if let Some(value) = fields.workspaces {
        permission.insert("workspaces".into(), json!(value.0));
    }
    if let Some(value) = fields.folder {
        permission.insert(
            "folder".into(),
            if value == "default" {
                Value::Null
            } else {
                json!(value)
            },
        );
    }
    if let Some(value) = fields.machine_access {
        permission.insert("machine_access".into(), json!(value));
    }
    json!({"operation":"set_permission","friendship_id":friendship_id,"permission":permission})
}

#[cfg(test)]
mod tests;
