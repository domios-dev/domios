use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};

mod tools;
use tools::{tool_args, tools};

const MAX_FRAME: u64 = 1024 * 1024;
const STARTUP_REFUSED: &str =
    "mcp_serve_authority_required: inherited tab credential and matching binding required";

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

pub fn serve(tab: &str, nonce: &str) {
    if let Err(reason) = validate_launch(tab, nonce, |key| std::env::var(key).ok()) {
        eprintln!("{reason}");
        std::process::exit(7);
    }
    let token = crate::nonempty_env("CODETERM_API_TOKEN").unwrap_or_default();
    let executable = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => {
            eprintln!("mcp_serve_unavailable: CLI executable unavailable");
            std::process::exit(7);
        }
    };
    let result = protocol(
        std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
        &token,
        crate::local_admission::host_scoped(),
        |args| {
            let mut command = std::process::Command::new(&executable);
            command.args(args);
            let output = codeterm_util::process_util::bounded_output(
                command,
                std::time::Duration::from_secs(60),
            )
            .map_err(|_| {
                "CLI outcome unavailable; check the receipt before replaying a mutation"
            })?;
            if !output.status.success() {
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

pub(super) fn protocol(
    mut input: impl BufRead,
    output: &mut impl Write,
    token: &str,
    scoped: bool,
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
        let result = if request["jsonrpc"] != "2.0" {
            Err((-32600, "Invalid JSON-RPC request"))
        } else {
            match request["method"].as_str() {
                Some("initialize") => Ok(
                    json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"domios","version":env!("CARGO_PKG_VERSION")}}),
                ),
                Some("tools/list") => Ok(json!({"tools":tools()})),
                Some("tools/call") => {
                    let call = serde_json::from_value::<ToolCall>(request["params"].clone());
                    match call
                        .map_err(|_| "Invalid tool call")
                        .and_then(|call| tool_args(&call.name, &call.arguments))
                    {
                        Err(reason) => Err((-32602, reason)),
                        Ok(args) => {
                            if !token.is_empty() && args.iter().any(|arg| arg.contains(token)) {
                                let response = json!({"jsonrpc":"2.0","id":null,"error":{"code":-32602,"message":"Credential material is not a tool argument"}});
                                writeln!(output, "{response}")?;
                                output.flush()?;
                                continue;
                            }
                            let cli = <crate::Cli as clap::Parser>::try_parse_from(
                                std::iter::once("domios".to_owned()).chain(args.clone()),
                            );
                            if cli.as_ref().is_err()
                                || (scoped
                                    && cli.as_ref().is_ok_and(|cli| {
                                        crate::local_admission::operation_for_cli(cli, true)
                                            .is_some()
                                    }))
                            {
                                Err((-32602, "Tool refused by CLI scope"))
                            } else {
                                match execute(&args) {
                                    Ok(text) => {
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
        scrub(&mut response, token);
        writeln!(output, "{response}")?;
        output.flush()?;
    }
}

#[cfg(test)]
mod tests;
