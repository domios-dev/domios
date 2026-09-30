use std::io::{IsTerminal, Read};

use serde_json::{json, Value};

use super::decision_render::{
    render_status, render_token_clear, render_token_set, render_token_status,
};
use super::transport::mem_transport_error;
use crate::cmd::print::{fail, fail_with, print_human, CliError};
use crate::{MemDecisionAction, MemDecisionTokenAction};

const TOKEN_ROUTE: &str = "/api/mem/decision/token";
const STATUS_ROUTE: &str = "/api/mem/decision/status";

/// The token enters through stdin only: an argv value would leak into shell history and `ps`.
pub(super) fn read_token(mut input: impl Read, is_terminal: bool) -> Result<String, String> {
    if is_terminal {
        return Err(
            "refusing to read the token from a terminal; pipe it in, e.g. \
             `pbpaste | codeterm mem decision token set --adapter typesafe-jev`"
                .into(),
        );
    }
    let mut raw = String::new();
    input
        .read_to_string(&mut raw)
        .map_err(|_| "failed to read the token from stdin".to_string())?;
    let token = raw.trim().to_string();
    if token.is_empty() {
        return Err("no token on stdin; pipe the token in".into());
    }
    Ok(token)
}

pub(super) struct Reply {
    status: u16,
    text: String,
}

impl Reply {
    #[cfg(test)]
    pub(super) fn for_test(status: u16, text: &str) -> Self {
        Self {
            status,
            text: text.into(),
        }
    }

    fn json(&self) -> Value {
        serde_json::from_str(&self.text).unwrap_or(Value::Null)
    }
}

fn exit_code_for(status: u16) -> i32 {
    match status {
        404 => 5,
        0 => 6,
        _ => 1,
    }
}

fn send(req: crate::local_api::LocalApiRequest, base: &str) -> Reply {
    match req.send() {
        Ok(response) => {
            let status = response.status().as_u16();
            Reply {
                status,
                text: response.text().unwrap_or_default(),
            }
        }
        Err(error) => Reply {
            status: 0,
            text: mem_transport_error(base, &error),
        },
    }
}

pub(super) fn error_message(reply: &Reply, secret: Option<&str>) -> String {
    let text = reply.text.trim();
    let message = reply
        .json()
        .pointer("/error/message")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| text.to_owned());
    let message = match secret {
        Some(secret) if !secret.is_empty() => message.replace(secret, "[redacted]"),
        _ => message,
    };
    if message.is_empty() {
        format!("HTTP {}", reply.status)
    } else {
        message
    }
}

fn ok_body_or_exit(reply: Reply, secret: Option<&str>) -> Value {
    if (200..300).contains(&reply.status) {
        return reply.json();
    }
    fail_with(
        exit_code_for(reply.status),
        &CliError {
            message: error_message(&reply, secret),
            code: None,
            data: Some(json!({ "http_status": reply.status })),
        },
    )
}

fn emit(json_mode: bool, body: &Value, human: impl FnOnce(&Value) -> String) {
    if json_mode {
        println!("{}", serde_json::to_string_pretty(body).unwrap_or_default());
    } else if body.is_object() {
        print!("{}", human(body));
    } else {
        print_human(body);
    }
}

fn token_url(base: &str, adapter: &str) -> String {
    format!(
        "{base}{TOKEN_ROUTE}?adapter={}",
        urlencoding::encode(adapter)
    )
}

pub(super) fn handle_mem_decision(json_mode: bool, action: MemDecisionAction) {
    let token = match &action {
        MemDecisionAction::Token {
            action: MemDecisionTokenAction::Set { .. },
        } => Some(
            read_token(std::io::stdin(), std::io::stdin().is_terminal())
                .unwrap_or_else(|message| fail(2, message)),
        ),
        _ => None,
    };

    let (base, _) = crate::read_api_config();
    let http = crate::build_local_api()
        .unwrap_or_else(|error| fail(1, format!("failed to build HTTP client: {error}")));
    let base = base.as_str();

    match action {
        MemDecisionAction::Status => {
            let body = ok_body_or_exit(send(http.get(format!("{base}{STATUS_ROUTE}")), base), None);
            emit(json_mode, &body, render_status);
        }
        MemDecisionAction::Token { action } => match action {
            MemDecisionTokenAction::Set { adapter } => {
                let token = token.unwrap_or_default();
                let request = http
                    .post(format!("{base}{TOKEN_ROUTE}"))
                    .json(&json!({ "adapter": adapter, "token": token }));
                let body = ok_body_or_exit(send(request, base), Some(&token));
                emit(json_mode, &body, render_token_set);
            }
            MemDecisionTokenAction::Clear { adapter } => {
                let body =
                    ok_body_or_exit(send(http.delete(token_url(base, &adapter)), base), None);
                emit(json_mode, &body, render_token_clear);
            }
            MemDecisionTokenAction::Status {
                adapter: Some(adapter),
            } => {
                let body = ok_body_or_exit(send(http.get(token_url(base, &adapter)), base), None);
                emit(json_mode, &body, render_token_status);
            }
            MemDecisionTokenAction::Status { adapter: None } => {
                let status =
                    ok_body_or_exit(send(http.get(format!("{base}{STATUS_ROUTE}")), base), None);
                let tokens: Vec<Value> = status
                    .get("adapters")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|adapter| adapter.get("token").cloned())
                    .collect();
                if json_mode {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&tokens).unwrap_or_default()
                    );
                } else if tokens.is_empty() {
                    println!("no decision adapters registered");
                } else {
                    for token in &tokens {
                        print!("{}", render_token_status(token));
                    }
                }
            }
        },
    }
}
