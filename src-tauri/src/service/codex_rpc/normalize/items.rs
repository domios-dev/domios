use crate::agent::chat_types::{ChatAttachment, ChatMsg, ChatSessionIntent, ChatTimelineMarker};
use serde_json::{json, Value};

fn text<'a>(item: &'a Value, key: &str) -> &'a str {
    item.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn output(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| serde_json::to_string_pretty(value).unwrap_or_default())
}

fn argument_summary(arguments: &Value) -> String {
    let summary = arguments
        .as_object()
        .map(|args| {
            args.iter()
                .map(|(key, value)| format!("{key}: {}", output(value)))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_else(|| output(arguments));
    let mut chars = summary.chars();
    let short: String = chars.by_ref().take(160).collect();
    if chars.next().is_some() {
        format!("{short}…")
    } else {
        short
    }
}

pub(super) fn outcome(item: &Value) -> Option<bool> {
    match text(item, "status") {
        "failed" | "declined" | "error" => Some(true),
        "completed" => Some(false),
        _ => None,
    }
}

pub(super) fn system_subtype(content: &str) -> Option<&'static str> {
    let content = content.trim();
    if content.starts_with("<turn_aborted>") && content.contains("</turn_aborted>") {
        Some("turn_aborted")
    } else if content
        .lines()
        .any(|line| line.starts_with("# AGENTS.md instructions for "))
        && content.contains("<INSTRUCTIONS>")
    {
        Some("agent_instructions")
    } else if content.starts_with("<environment_context")
        && content.ends_with("</environment_context>")
    {
        Some("environment_context")
    } else {
        None
    }
}

pub(super) fn from_item(item: &Value, previous: Option<&ChatMsg>) -> Option<ChatMsg> {
    let kind = text(item, "type");
    let mut message = previous.cloned().unwrap_or_default();
    message.kind = "tool_call".into();
    message.tool_id = Some(text(item, "id").into());
    message.tool_name = Some(kind.into());
    message.tool_kind = Some("generic".into());
    message.tool_input = Some(item.clone());
    message.tool_error = outcome(item);
    message.tool_duration_ms = item.get("durationMs").and_then(Value::as_u64);
    message.collapsed = true;
    match kind {
        "mcpToolCall" | "dynamicToolCall" => {
            let server = text(
                item,
                if kind == "mcpToolCall" {
                    "server"
                } else {
                    "namespace"
                },
            );
            let tool = text(item, "tool");
            message.tool_name = Some(if server.is_empty() {
                tool.into()
            } else {
                format!("{server} · {tool}")
            });
            message.tool_title = message.tool_name.clone();
            let arguments = item.get("arguments").cloned().unwrap_or(Value::Null);
            message.tool_summary = Some(argument_summary(&arguments));
            message.tool_args = message.tool_summary.clone();
            message.tool_input = Some(arguments);
            if let Some(result) = item
                .get("result")
                .or_else(|| item.get("contentItems"))
                .filter(|v| !v.is_null())
            {
                message.tool_result = Some(tool_output(result));
                if let Some(error) = result.get("isError").and_then(Value::as_bool) {
                    message.tool_error = Some(error);
                }
            }
            if let Some(error) = item.get("error").filter(|v| !v.is_null()) {
                message.tool_result = Some(output(error));
                message.tool_error = Some(true);
            }
            if item.get("success") == Some(&Value::Bool(false)) {
                message.tool_error = Some(true);
            }
        }
        "functionCallOutput" => {
            message.kind = "tool_result".into();
            message.tool_name = Some(text(item, "name").into());
            message.tool_result = item.get("output").map(output);
        }
        "webSearch" => {
            message.tool_kind = Some("web".into());
            message.tool_summary = Some(text(item, "query").into());
            message.tool_rich_result = Some(json!({"kind":"web", "query":item["query"],
                "action":item["action"]["type"], "url":item["action"]["url"],
                "pattern":item["action"]["pattern"], "resultCount":item["results"].as_array().map(Vec::len)}));
            message.tool_result = item.get("results").filter(|v| !v.is_null()).map(output);
        }
        "imageView" | "imageGeneration" => {
            let generated = kind == "imageGeneration";
            let path = text(item, if generated { "savedPath" } else { "path" });
            message.tool_kind = Some("image".into());
            message.tool_summary = Some(
                if path.is_empty() {
                    text(item, "revisedPrompt")
                } else {
                    path
                }
                .into(),
            );
            message.tool_rich_result = Some(
                json!({"kind":"image", "origin":if generated { "generated" } else { "viewed" },
                "path":path,"prompt":item["revisedPrompt"],"status":item["status"]}),
            );
            message.tool_result = item
                .get("result")
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
                .map(|result| {
                    if generated && !result.starts_with("data:") {
                        format!("data:image/png;base64,{result}")
                    } else {
                        result.into()
                    }
                });
        }
        "plan" => {
            message.kind = "assistant".into();
            message.content = text(item, "text").into();
        }
        "planUpdate" => {
            message.tool_name = Some("update_plan".into());
            message.tool_summary = Some(text(item, "explanation").into());
            message.tool_result = Some(output(&item["plan"]));
            message.tool_error = Some(false);
        }
        "collabAgentToolCall" | "subAgentActivity" => {
            let agent = if kind == "subAgentActivity" {
                text(item, "agentThreadId")
            } else {
                item["receiverThreadIds"][0]
                    .as_str()
                    .unwrap_or_else(|| text(item, "id"))
            };
            let state = if kind == "subAgentActivity" {
                text(item, "kind")
            } else {
                item["agentsStates"][agent]["status"]
                    .as_str()
                    .unwrap_or_default()
            };
            let state = match state {
                "pendingInit" => "started",
                "errored" => "failed",
                "shutdown" => "closed",
                other => other,
            };
            message.tool_kind = Some("agent".into());
            message.tool_name = Some("agent_activity".into());
            message.tool_summary = Some(agent.into());
            message.tool_input = Some(
                json!({"agent_id":agent,"agent_path":item["agentPath"],"agent_state":state,
                "agent_call_id":item["id"],"description":item["prompt"],"model":item["model"],
                "result":item["agentsStates"][agent]["message"]}),
            );
        }
        "sleep" => {
            message.tool_summary = Some(format!("Wait {} ms", item["durationMs"]));
        }
        "hookPrompt" | "enteredReviewMode" | "exitedReviewMode" | "contextCompaction" | "error" => {
            message.kind = "system".into();
            message.system_subtype = Some(kind.into());
            message.content = match kind {
                "hookPrompt" => item["fragments"]
                    .as_array()
                    .map(|parts| {
                        parts
                            .iter()
                            .filter_map(|part| part.get("text").and_then(Value::as_str))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default(),
                "contextCompaction" => "context compacted".into(),
                "error" => text(item, "message").into(),
                _ => text(item, "review").into(),
            };
            if kind == "contextCompaction" {
                message.timeline_marker = Some(ChatTimelineMarker::ContextCompaction);
            }
        }
        "toolFlow" => {
            message.kind = "tool_flow".into();
            message.content = text(item, "state").into();
        }
        "sessionIntent" => {
            message.kind = "custom".into();
            message.session_intent = Some(ChatSessionIntent {
                title: item.get("title").and_then(Value::as_str).map(str::to_owned),
                goal: item
                    .get("objective")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            });
        }
        _ => return None,
    }
    Some(message)
}

fn tool_output(value: &Value) -> String {
    let mut value = value.clone();
    let parts = if value.is_array() {
        value.as_array_mut()
    } else {
        value.get_mut("content").and_then(Value::as_array_mut)
    };
    if let Some(parts) = parts {
        for part in parts {
            match part["type"].as_str() {
                Some("image") => {
                    let url = format!(
                        "data:{};base64,{}",
                        text(part, "mimeType"),
                        text(part, "data")
                    );
                    *part = json!({"type":"image_url","image_url":{"url":url}});
                }
                Some("inputImage") => {
                    *part = json!({"type":"image_url","image_url":{"url":part["imageUrl"]}})
                }
                _ => {}
            }
        }
    }
    output(&value)
}

pub(super) fn attachments(item: &Value) -> Option<Vec<ChatAttachment>> {
    let attachments: Vec<_> = item["content"]
        .as_array()?
        .iter()
        .enumerate()
        .filter_map(|(index, part)| {
            let kind = text(part, "type");
            if !matches!(kind, "image" | "localImage") {
                return None;
            }
            let url = text(part, "url");
            let (mime_type, data) = url
                .strip_prefix("data:")
                .and_then(|s| s.split_once(";base64,"))
                .map(|(mime, data)| (mime.into(), Some(data.into())))
                .unwrap_or_else(|| ("image/png".into(), None));
            Some(ChatAttachment {
                id: format!("{}-image-{index}", text(item, "id")),
                mime_type,
                data,
                url: (!url.is_empty() && !url.starts_with("data:")).then(|| url.into()),
                path: part.get("path").and_then(Value::as_str).map(str::to_owned),
            })
        })
        .collect();
    (!attachments.is_empty()).then_some(attachments)
}

pub(super) fn notification_item(method: &str, params: &Value) -> Option<Value> {
    if method == "thread/goal/updated" {
        return Some(
            json!({"id":"thread:goal","type":"sessionIntent","objective":params["goal"]["objective"]}),
        );
    }
    let turn = params
        .get("turnId")
        .or_else(|| params.pointer("/turn/id"))?
        .as_str()?;
    match method {
        "turn/started" | "turn/completed" => {
            Some(json!({"id":format!("{turn}:{method}"),"type":"toolFlow",
            "state":if method == "turn/started" { "start" } else { "end" }}))
        }
        "turn/plan/updated" => Some(json!({"id":format!("{turn}:plan"),"type":"planUpdate",
            "plan":params["plan"],"explanation":params["explanation"]})),
        "error" => Some(
            json!({"id":format!("{turn}:error"),"type":"error","message":params["error"]["message"]}),
        ),
        _ => None,
    }
}
