use super::opencode_sse::SessionUpdate;
use crate::agent::chat_types::ChatMsg;

pub fn message(session_id: &str, update: &SessionUpdate) -> ChatMsg {
    let (key, content) = match update {
        SessionUpdate::Compacted => ("compacted", "Session context compacted.".into()),
        SessionUpdate::Error { error } => {
            let detail = error
                .pointer("/data/message")
                .or_else(|| error.get("message"))
                .and_then(serde_json::Value::as_str)
                .map(String::from)
                .unwrap_or_else(|| error.to_string());
            ("error", format!("Session error: {detail}"))
        }
        SessionUpdate::Todos { todos } if todos.is_empty() => ("todos", "No pending tasks.".into()),
        SessionUpdate::Todos { todos } => (
            "todos",
            todos
                .iter()
                .filter_map(|todo| {
                    let content = todo.get("content")?.as_str()?;
                    let status = todo
                        .get("status")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("pending");
                    Some(format!("- [{status}] {content}"))
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    };
    ChatMsg {
        id: format!("session:{session_id}:{key}"),
        kind: "system".into(),
        content,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todo_updates_replace_the_same_session_row() {
        let todo = |status: &str| SessionUpdate::Todos {
            todos: vec![serde_json::json!({"content":"task", "status":status})],
        };
        let pending = message("own", &todo("pending"));
        let done = message("own", &todo("completed"));
        assert_eq!(pending.id, done.id);
        assert_ne!(pending.id, message("foreign", &todo("completed")).id);
        assert_eq!(done.content, "- [completed] task");
    }

    #[test]
    fn errors_preserve_the_server_detail() {
        let error = SessionUpdate::Error {
            error: serde_json::json!({"data":{"message":"opaque failure"}}),
        };
        assert!(message("own", &error).content.contains("opaque failure"));
    }
}
