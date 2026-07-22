use crate::client::SocketClient;
use crate::cmd::print::handle_result;
use crate::BridgeAction;

pub mod migrate;

pub(crate) fn bridge_text(text: String, raw_text: bool) -> String {
    if raw_text {
        text
    } else {
        text.replace(r"\n", "\n")
    }
}

pub fn handle_bridge(client: &mut SocketClient, json: bool, action: BridgeAction) {
    match action {
        BridgeAction::Restart => {
            let result = client.call("bridge.restart", None);
            handle_result(json, result);
        }
        BridgeAction::Send {
            text,
            raw_text,
            service,
            to,
            append,
            r#type,
            file,
            caption,
            filename,
            buttons,
        } => {
            let mut params = serde_json::json!({
                "service": service,
                "chat_id": to,
                "type": r#type,
                "append": append,
            });
            if let Some(t) = text {
                params["text"] = serde_json::Value::String(bridge_text(t, raw_text));
            }
            if let Some(f) = file {
                params["file"] = serde_json::Value::String(f);
            }
            if let Some(c) = caption {
                params["caption"] = serde_json::Value::String(c);
            }
            if let Some(n) = filename {
                params["filename"] = serde_json::Value::String(n);
            }
            if let Some(b) = buttons {
                match serde_json::from_str::<serde_json::Value>(&b) {
                    Ok(v) => {
                        params["buttons"] = v;
                    }
                    Err(e) => {
                        eprintln!("Invalid buttons JSON: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            let result = client.call("bridge.send", Some(params));
            handle_result(json, result);
        }
        BridgeAction::React {
            to,
            message_id,
            emoji,
        } => {
            let result = client.call(
                "bridge.react",
                Some(serde_json::json!({
                    "chat_id": to,
                    "message_id": message_id,
                    "emoji": emoji,
                })),
            );
            handle_result(json, result);
        }
        BridgeAction::Migrate {
            to,
            service,
            announce,
            announce_chat_id,
            no_rollback,
            force,
            claim_timeout_ms,
        } => migrate::run_migrate(
            json,
            migrate::MigrateArgs {
                to,
                service,
                announce,
                announce_chat_id,
                no_rollback,
                force,
                claim_timeout_ms,
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::bridge_text;

    #[test]
    fn quoted_newline_reaches_bridge_as_newline() {
        let payload = serde_json::json!({
            "text": bridge_text("first\\nsecond".to_string(), false),
            "chat_id": 42,
            "service": "telegram",
        });

        assert_eq!(
            payload,
            serde_json::json!({
                "text": "first\nsecond",
                "chat_id": 42,
                "service": "telegram",
            })
        );
    }

    #[test]
    fn raw_text_preserves_literal_newline_escape() {
        assert_eq!(
            bridge_text("first\\nsecond".to_string(), true),
            r"first\nsecond"
        );
    }
}
