use std::io::{IsTerminal, Read};

use serde_json::{json, Value};

use crate::args::supporter::{SupporterAction, SupporterTokenAction};
use crate::cmd::mem::transport::mem_transport_error;
use crate::cmd::print::{fail, fail_with, print_human, CliError};
use crate::cmd::supporter_render::{
    render_models, render_status, render_token_clear, render_token_set, render_token_status,
};

const TOKEN_ROUTE: &str = "/api/supporter/token";
const STATUS_ROUTE: &str = "/api/supporter/status";
const MODELS_ROUTE: &str = "/api/supporter/models";
const USE_ROUTE: &str = "/api/supporter/use";

/// The token enters through stdin only: an argv value would leak into shell history and `ps`.
pub(super) fn read_token(mut input: impl Read, is_terminal: bool) -> Result<String, String> {
    if is_terminal {
        return Err(
            "refusing to read the token from a terminal; pipe it in, e.g. \
             `pbpaste | codeterm supporter token set`"
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
        Ok(response) => Reply {
            status: response.status().as_u16(),
            text: response.text().unwrap_or_default(),
        },
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

fn token_url(base: &str, adapter: Option<&str>) -> String {
    match adapter {
        Some(adapter) => format!(
            "{base}{TOKEN_ROUTE}?adapter={}",
            urlencoding::encode(adapter)
        ),
        None => format!("{base}{TOKEN_ROUTE}"),
    }
}

fn status_request(http: &crate::local_api::LocalApiClient, base: &str) -> Value {
    ok_body_or_exit(send(http.get(format!("{base}{STATUS_ROUTE}")), base), None)
}

pub fn handle_supporter(json_mode: bool, action: SupporterAction) {
    let (base, _) = crate::read_api_config();
    let http = crate::build_local_api()
        .unwrap_or_else(|error| fail(1, format!("failed to build HTTP client: {error}")));
    let base = base.as_str();

    match action {
        SupporterAction::Status => {
            let status = status_request(&http, base);
            emit(json_mode, &status, render_status);
        }
        SupporterAction::Models => {
            let body = ok_body_or_exit(send(http.get(format!("{base}{MODELS_ROUTE}")), base), None);
            emit(json_mode, &body, render_models);
        }
        SupporterAction::Use { adapter, model } => {
            let body = json!({ "adapter": adapter, "model": model });
            let reply = send(http.post(format!("{base}{USE_ROUTE}")).json(&body), base);
            let selected = ok_body_or_exit(reply, None);
            emit(json_mode, &selected, |value| {
                if value["state"] == "saved_not_active" {
                    return format!(
                        "saved, not active: {}. Retry with codeterm supporter use {}\n",
                        adapter, adapter
                    );
                }
                format!(
                    "active {}{}\n",
                    value.get("adapter").and_then(Value::as_str).unwrap_or("?"),
                    value
                        .get("model")
                        .and_then(Value::as_str)
                        .map(|model| format!(" / {model}"))
                        .unwrap_or_default()
                )
            });
        }
        SupporterAction::Disconnect { adapter } => {
            let reply = send(
                http.delete(format!(
                    "{base}/api/supporter/connections/{}",
                    urlencoding::encode(&adapter)
                )),
                base,
            );
            let removed = ok_body_or_exit(reply, None);
            emit(json_mode, &removed, |value| {
                format!(
                    "removed {}. Active adapter: {}\n",
                    adapter,
                    value["active"].as_str().unwrap_or("none")
                )
            });
        }
        SupporterAction::Token { action } => match action {
            SupporterTokenAction::Set { adapter } => {
                let token_status = send(http.get(token_url(base, adapter.as_deref())), base);
                let current = ok_body_or_exit(token_status, None);
                if current.get("token_mode").and_then(Value::as_str) == Some("none") {
                    emit(json_mode, &current, render_token_set);
                    return;
                }
                let token = read_token(std::io::stdin(), std::io::stdin().is_terminal())
                    .unwrap_or_else(|message| {
                        fail_with(
                            2,
                            &CliError {
                                message,
                                code: Some(-32602),
                                data: None,
                            },
                        )
                    });
                let mut body = json!({ "token": token });
                if let Some(adapter) = adapter {
                    body["adapter"] = json!(adapter);
                }
                let reply = send(http.post(format!("{base}{TOKEN_ROUTE}")).json(&body), base);
                let saved = ok_body_or_exit(reply, Some(&token));
                emit(json_mode, &saved, render_token_set);
            }
            SupporterTokenAction::Clear { adapter } => {
                let reply = send(http.delete(token_url(base, adapter.as_deref())), base);
                let removed = ok_body_or_exit(reply, None);
                emit(json_mode, &removed, render_token_clear);
            }
            SupporterTokenAction::Status {
                adapter: Some(adapter),
            } => {
                let reply = send(http.get(token_url(base, Some(&adapter))), base);
                let status = ok_body_or_exit(reply, None);
                emit(json_mode, &status, render_token_status);
            }
            SupporterTokenAction::Status { adapter: None } => {
                let status = status_request(&http, base);
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
                    println!("no Supporter Agent adapters registered");
                } else {
                    for token in &tokens {
                        print!("{}", render_token_status(token));
                    }
                }
            }
        },
    }
}

#[cfg(test)]
mod tests;
