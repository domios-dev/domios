use super::{protocol_with_tools, NoInbound, ToolCall};
use crate::client::{SocketCallError, SocketClient};
use codeterm_plugin::agent_runtime::NEIGHBOR_REPLY_MCP_TOOL;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{ffi::OsStr, io};

const RUNTIME_ROUTE: &str = "/api/account/neighbors/runtime";

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Answer,
    Refusal,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    in_reply_to: String,
    kind: Kind,
    body: String,
}

struct Launch<'a> {
    tab: &'a str,
    nonce: &'a str,
    friendship: &'a str,
    socket: String,
    token: String,
}

impl Launch<'_> {
    fn params(&self, reply: Reply) -> Value {
        let kind = match reply.kind {
            Kind::Answer => "answer",
            Kind::Refusal => "refusal",
        };
        let body = json!({"operation":"send","friendship_id":self.friendship,
            "kind":kind,"in_reply_to":reply.in_reply_to,"body":reply.body});
        json!({"method":"POST","path":RUNTIME_ROUTE,"caller_tab_id":self.tab,
            "session_binding_nonce":self.nonce,"headers":{"content-type":"application/json"},
            "body":body.to_string()})
    }

    fn send(&self, reply: Reply) -> Result<String, &'static str> {
        let mut client = SocketClient::connect_bound_socket(&self.socket, &self.token)
            .map_err(|_| "Neighbor reply socket unavailable before submission")?;
        let result = client
            .call_detailed("api.request", Some(self.params(reply)))
            .map_err(|error| match error {
                SocketCallError::Rpc(_) => "Neighbor reply refused by the daemon",
                SocketCallError::Transport(_) => {
                    "Neighbor reply outcome unavailable. Check the receipt before replaying."
                }
            })?;
        if !result["status"]
            .as_u64()
            .is_some_and(|status| (200..300).contains(&status))
        {
            return Err("Neighbor reply refused by the daemon");
        }
        let receipt = result["body"]
            .as_str()
            .and_then(|body| serde_json::from_str::<Value>(body).ok())
            .ok_or("Neighbor reply receipt unavailable. Check the receipt before replaying.")?;
        if receipt["submission"] == "unconfirmed" {
            return Ok(format!(
                "Neighbor reply submission is unconfirmed. Do not replay it. {receipt}"
            ));
        }
        if !receipt["message_id"].is_string() {
            return Err("Neighbor reply receipt unavailable. Check the receipt before replaying.");
        }
        Ok(receipt.to_string())
    }
}

fn is_hex_id(id: &str, len: usize) -> bool {
    id.len() == len
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn tools() -> Vec<Value> {
    vec![
        json!({"name":NEIGHBOR_REPLY_MCP_TOOL,"description":"Send a linked answer or refusal to a question from this concierge's bound Neighbor.",
        "inputSchema":{"type":"object","required":["in_reply_to","kind","body"],"additionalProperties":false,
        "properties":{"in_reply_to":{"type":"string","pattern":"^[0-9a-f]{32}$"},
        "kind":{"type":"string","enum":["answer","refusal"]},"body":{"type":"string","minLength":1}}}}),
    ]
}

fn protocol(
    input: impl io::BufRead,
    output: &mut impl io::Write,
    token: &str,
    mut send: impl FnMut(Reply) -> Result<String, &'static str>,
) -> io::Result<()> {
    protocol_with_tools(
        input,
        output,
        token,
        &mut NoInbound,
        tools,
        |call: ToolCall| {
            if call.name != NEIGHBOR_REPLY_MCP_TOOL {
                return Err("Tool is not allowlisted");
            }
            let reply: Reply = serde_json::from_value(call.arguments)
                .map_err(|_| "Invalid linked reply arguments")?;
            if !is_hex_id(&reply.in_reply_to, 32) || reply.body.trim().is_empty() {
                return Err("A linked reply requires a valid question ID and nonempty text");
            }
            if !token.is_empty() && reply.body.contains(token) {
                return Err("Credential material is not a tool argument");
            }
            Ok(send(reply))
        },
    )
}

pub(super) fn serve(tab: &str, nonce: &str, friendship: &OsStr) {
    let launch = (|| {
        let friendship = friendship.to_str().filter(|id| is_hex_id(id, 64))?;
        Some(Launch {
            tab,
            nonce,
            friendship,
            socket: crate::nonempty_env("CODETERM_SOCKET_PATH")?,
            token: crate::nonempty_env("CODETERM_API_TOKEN")?,
        })
    })();
    let Some(launch) = launch else {
        eprintln!("Neighbor reply MCP requires a local socket and bound friendship");
        std::process::exit(7);
    };
    if protocol(
        io::stdin().lock(),
        &mut io::stdout().lock(),
        &launch.token,
        |reply| launch.send(reply),
    )
    .is_err()
    {
        eprintln!("Neighbor reply MCP stdio transport ended");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests;
