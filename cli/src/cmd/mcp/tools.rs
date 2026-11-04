use serde_json::{json, Value};

struct Tool {
    name: &'static str,
    description: &'static str,
    command: &'static [&'static str],
    required: &'static [&'static str],
    optional: &'static [&'static str],
    positional: Option<&'static str>,
}
const TOOLS: &[Tool] = &[
    Tool {
        name: "send",
        description: "Send a pad payload as this tab to an authorized target tab",
        command: &["send"],
        required: &["target_tab", "from_pad"],
        optional: &[],
        positional: None,
    },
    Tool {
        name: "agent_report",
        description: "Report as this tab using a Markdown file in its pad",
        command: &["agent", "report"],
        required: &["from_pad"],
        optional: &["status", "sha"],
        positional: None,
    },
    Tool {
        name: "agent_startup_ack",
        description: "Acknowledge this tab's own launch",
        command: &["agent", "startup-ack"],
        required: &[],
        optional: &[],
        positional: None,
    },
    Tool {
        name: "mem_search",
        description: "Search Domios memory",
        command: &["mem", "search"],
        required: &["query"],
        optional: &["area", "kind"],
        positional: Some("query"),
    },
    Tool {
        name: "mem_get",
        description: "Read one Domios memory record",
        command: &["mem", "get"],
        required: &["id"],
        optional: &[],
        positional: Some("id"),
    },
    Tool {
        name: "task_show",
        description: "Read one Domios task",
        command: &["task", "show"],
        required: &["id"],
        optional: &[],
        positional: Some("id"),
    },
    Tool {
        name: "notify_send",
        description: "Notify the owner from this tab using a pad payload",
        command: &["notify", "send"],
        required: &["title", "from_pad", "level"],
        optional: &["severity"],
        positional: Some("title"),
    },
];

pub(super) fn tools() -> Vec<Value> {
    TOOLS.iter().map(|tool| {
        let properties: serde_json::Map<String,Value> = tool.required.iter().chain(tool.optional).map(|key| ((*key).into(), json!({"type":"string","minLength":1,"maxLength":4096}))).collect();
        json!({"name":tool.name,"description":tool.description,"inputSchema":{"type":"object","properties":properties,"required":tool.required,"additionalProperties":false}})
    }).collect()
}

pub(super) fn tool_args(name: &str, arguments: &Value) -> Result<Vec<String>, &'static str> {
    let tool = TOOLS
        .iter()
        .find(|tool| tool.name == name)
        .ok_or("Tool is not allowlisted")?;
    let values = arguments
        .as_object()
        .ok_or("Tool arguments must be an object")?;
    if values
        .keys()
        .any(|key| !tool.required.contains(&key.as_str()) && !tool.optional.contains(&key.as_str()))
        || tool.required.iter().any(|key| !values.contains_key(*key))
        || values.values().any(|value| {
            value.as_str().is_none_or(|text| {
                text.trim().is_empty() || text.len() > 4096 || text.contains('\0')
            })
        })
    {
        return Err("Invalid tool arguments");
    }
    if values
        .get("target_tab")
        .and_then(Value::as_str)
        .is_some_and(|target| {
            target.starts_with('-')
                || !target
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
        })
    {
        return Err("MCP sends require one local target tab");
    }
    if values.get("from_pad").is_some_and(|value| value == "-") {
        return Err("MCP pad inputs require a file");
    }
    let mut args = vec!["--json".into()];
    args.extend(tool.command.iter().map(|part| (*part).to_owned()));
    for key in tool.required.iter().chain(tool.optional) {
        if Some(*key) == tool.positional {
            continue;
        }
        if let Some(value) = values.get(*key).and_then(Value::as_str) {
            let flag = if *key == "target_tab" {
                "tab".into()
            } else {
                key.replace('_', "-")
            };
            args.push(format!("--{flag}={value}"));
        }
    }
    if let Some(value) = tool
        .positional
        .and_then(|key| values.get(key))
        .and_then(Value::as_str)
    {
        args.push("--".into());
        args.push(value.into());
    }
    Ok(args)
}
