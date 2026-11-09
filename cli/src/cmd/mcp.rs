use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};

mod tools;
#[cfg(test)]
use crate::cmd::inbound::NoInbound;
use crate::cmd::inbound::{self, Inbound};
use tools::{is_held_receipt, is_messages_check, launch_args, tool_args, tools, MESSAGES_CHECKED};

const MAX_FRAME: u64 = 1024 * 1024;
const STARTUP_REFUSED: &str =
    "mcp_serve_authority_required: tab credential and matching binding required";
const MESH_REFUSED: &str = "mcp_serve_local_only: MCP stdio serves only the launching tab";

pub(super) fn refuse_mesh_target(mesh: Option<&str>) -> Result<(), &'static str> {
    mesh.map_or(Ok(()), |_| Err(MESH_REFUSED))
}

fn negotiated_version(offered: Option<&str>) -> &str {
    match offered {
        Some(version @ ("2025-06-18" | "2025-03-26" | "2024-11-05")) => version,
        _ => "2025-06-18",
    }
}

pub(super) fn validate_launch(
    tab: &str,
    nonce: &str,
    env: impl Fn(&str) -> Option<String>,
) -> Result<(), &'static str> {
    for key in [
        "CODETERM_API_TOKEN",
        "CODETERM_API_URL",
        "CODETERM_TAB_ID",
        "CODETERM_SESSION_BINDING_NONCE",
    ] {
        if env(key).is_none_or(|value| value.trim().is_empty()) {
            return Err(STARTUP_REFUSED);
        }
    }
    if env("CODETERM_TAB_ID").as_deref() != Some(tab)
        || env("CODETERM_SESSION_BINDING_NONCE").as_deref() != Some(nonce)
    {
        return Err(STARTUP_REFUSED);
    }
    Ok(())
}

pub fn serve(tab: &str, nonce: &str, mesh: Option<&str>) {
    if let Err(reason) = refuse_mesh_target(mesh)
        .and_then(|()| validate_launch(tab, nonce, |key| std::env::var(key).ok()))
    {
        eprintln!("{reason}");
        std::process::exit(7);
    }
    let token = crate::nonempty_env("CODETERM_API_TOKEN").unwrap_or_default();
    let task_bound = crate::nonempty_env("CODETERM_TASK_ID").is_some();
    let executable = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => {
            eprintln!("mcp_serve_unavailable: CLI executable unavailable");
            std::process::exit(7);
        }
    };
    let result = protocol_with(
        std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
        &token,
        &mut inbound::Daemon::new(tab, Some(nonce)),
        |args| {
            let mut command = std::process::Command::new(&executable);
            command.args(launch_args(args.to_vec(), task_bound));
            let output = codeterm_util::process_util::bounded_output(
                command,
                std::time::Duration::from_secs(60),
            )
            .map_err(|_| {
                "CLI outcome unavailable; check the receipt before replaying a mutation"
            })?;
            if !output.status.success() && !is_held_receipt(&output.stdout) {
                return Err("CLI operation refused or failed; inspect the tab's own task evidence");
            }
            if output.stdout.len() > MAX_FRAME as usize {
                return Err("CLI result exceeds MCP bounds");
            }
            String::from_utf8(output.stdout).map_err(|_| "CLI result is not UTF-8")
        },
    );
    if result.is_err() {
        eprintln!("mcp_serve_io_error: stdio transport ended");
        std::process::exit(1);
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolCall {
    name: String,
    #[serde(default = "empty_object")]
    arguments: Value,
    #[serde(rename = "_meta", default)]
    _meta: Option<Value>,
}
fn empty_object() -> Value {
    json!({})
}

fn scrub(value: &mut Value, token: &str) {
    if token.is_empty() {
        return;
    }
    match value {
        Value::String(text) => *text = text.replace(token, "[redacted]"),
        Value::Array(values) => values.iter_mut().for_each(|value| scrub(value, token)),
        Value::Object(values) => {
            let old = std::mem::take(values);
            for (key, mut value) in old {
                scrub(&mut value, token);
                values.insert(key.replace(token, "[redacted]"), value);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
pub(super) fn protocol(
    input: impl BufRead,
    output: &mut impl Write,
    token: &str,
    execute: impl FnMut(&[String]) -> Result<String, &'static str>,
) -> std::io::Result<()> {
    protocol_with(input, output, token, &mut NoInbound, execute)
}

pub(super) fn protocol_with(
    mut input: impl BufRead,
    output: &mut impl Write,
    token: &str,
    inbound: &mut impl Inbound,
    mut execute: impl FnMut(&[String]) -> Result<String, &'static str>,
) -> std::io::Result<()> {
    loop {
        let mut line = String::new();
        let size = input.by_ref().take(MAX_FRAME + 1).read_line(&mut line)?;
        if size == 0 {
            return Ok(());
        }
        if size as u64 > MAX_FRAME {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "MCP frame exceeds bounds",
            ));
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => {
                writeln!(
                    output,
                    "{}",
                    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Invalid JSON"}})
                )?;
                output.flush()?;
                continue;
            }
        };
        let Some(id) = request.get("id") else {
            continue;
        };
        let mut carries_messages = false;
        let result = if request["jsonrpc"] != "2.0" {
            Err((-32600, "Invalid JSON-RPC request"))
        } else {
            match request["method"].as_str() {
                Some("initialize") => Ok(
                    json!({"protocolVersion":negotiated_version(request["params"]["protocolVersion"].as_str()),"capabilities":{"tools":{}},"serverInfo":{"name":"domios","version":env!("CARGO_PKG_VERSION")}}),
                ),
                Some("tools/list") => Ok(json!({"tools":tools()})),
                Some("tools/call") => {
                    let call = serde_json::from_value::<ToolCall>(request["params"].clone());
                    let checks_messages = call
                        .as_ref()
                        .is_ok_and(|call| is_messages_check(&call.name, &call.arguments));
                    match call
                        .map_err(|_| "Invalid tool call")
                        .and_then(|call| tool_args(&call.name, &call.arguments))
                    {
                        Err(_) if checks_messages => {
                            carries_messages = true;
                            Ok(json!({"content":[{"type":"text","text":MESSAGES_CHECKED}]}))
                        }
                        Err(reason) => Err((-32602, reason)),
                        Ok(args) => {
                            if !token.is_empty() && args.iter().any(|arg| arg.contains(token)) {
                                Err((-32602, "Credential material is not a tool argument"))
                            } else if <crate::Cli as clap::Parser>::try_parse_from(
                                std::iter::once("domios".to_owned()).chain(args.clone()),
                            )
                            .is_err()
                            {
                                Err((-32602, "Invalid CLI tool arguments"))
                            } else {
                                match execute(&args) {
                                    Ok(text) => {
                                        carries_messages = true;
                                        Ok(json!({"content":[{"type":"text","text":text}]}))
                                    }
                                    Err(reason) => Ok(
                                        json!({"isError":true,"content":[{"type":"text","text":reason}]}),
                                    ),
                                }
                            }
                        }
                    }
                }
                _ => Err((-32601, "Method not available")),
            }
        };
        let mut response = match result {
            Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
            Err((code, message)) => {
                json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
            }
        };
        let claim = carries_messages.then(|| inbound.claim()).flatten();
        if let Some(claim) = &claim {
            if let Some(content) = response["result"]["content"].as_array_mut() {
                content.push(json!({"type":"text","text":claim.text}));
            }
        }
        scrub(&mut response, token);
        // A failed write is left unsettled: bytes may have gone out, so the daemon never replays it.
        writeln!(output, "{response}")?;
        output.flush()?;
        if let Some(claim) = &claim {
            inbound.settle(&claim.id, true);
        }
    }
}

#[cfg(test)]
mod tests;
