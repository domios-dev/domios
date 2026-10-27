use crate::args::account::NeighborAction;
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
    let command = match action {
        NeighborAction::List => json!({"operation":"list"}),
        NeighborAction::Status => json!({"operation":"status"}),
        NeighborAction::Enroll { confirm_takeover } => {
            json!({"operation":"enroll","takeover_confirmed":confirm_takeover})
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
            from_pad,
            allow_tab_requests,
        } => {
            let scope = authored(&from_pad);
            json!({"operation":"set_permission","friendship_id":friendship_id,"permission":{"allowed":true,"scope":scope.as_str(),"allow_tab_requests":allow_tab_requests}})
        }
        NeighborAction::Deny { friendship_id } => {
            json!({"operation":"set_permission","friendship_id":friendship_id,"permission":{"allowed":false,"scope":"","allow_tab_requests":false}})
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
    let client =
        crate::build_local_api().unwrap_or_else(|_| fail(1, "Local Neighbor service unavailable"));
    let response = client
        .post(if listing {
            "/api/account/neighbors"
        } else {
            "/api/account/neighbors/runtime"
        })
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
                    text("status"),
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

#[cfg(test)]
mod tests;
