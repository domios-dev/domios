use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeHttpSse {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<RuntimeHttpSse>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub completion_events: bool,
    pub readiness_prefix: String,
    pub password_env: String,
    pub username: String,
    pub username_env: String,
    pub event_path: String,
    pub event_method: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub request_events: BTreeMap<String, RuntimeHttpRequestEvent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub response_headers: Vec<String>,
    pub routes: BTreeMap<String, RuntimeHttpRoute>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct RuntimeHttpRoute {
    pub method: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeHttpRequestEvent {
    pub type_pointer: String,
    pub id_pointer: String,
    pub thread_pointer: String,
    pub reply_method: String,
    pub reject_method: String,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub reject_body: serde_json::Value,
    pub path_params: BTreeMap<String, String>,
}

impl RuntimeHttpSse {
    pub fn valid_for(&self, capability: &super::AgentRuntimeCapability) -> bool {
        self.alternatives.len() <= 4
            && self
                .alternatives
                .iter()
                .enumerate()
                .all(|(index, candidate)| {
                    candidate.alternatives.is_empty()
                        && candidate.completion_events == self.completion_events
                        && candidate.username == self.username
                        && candidate.readiness_prefix != self.readiness_prefix
                        && self.alternatives[..index]
                            .iter()
                            .all(|previous| previous.readiness_prefix != candidate.readiness_prefix)
                        && candidate.valid_transport(capability)
                })
            && self.valid_transport(capability)
            && capability.request_methods.values().flatten().all(|method| {
                self.routes.contains_key(method)
                    || self
                        .alternatives
                        .iter()
                        .any(|candidate| candidate.routes.contains_key(method))
            })
    }

    fn valid_transport(&self, capability: &super::AgentRuntimeCapability) -> bool {
        let bounded = |value: &str, limit| {
            !value.is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
        };
        bounded(&self.readiness_prefix, 128)
            && bounded(&self.username, 128)
            && !self.username.contains(':')
            && bounded(&self.username_env, 128)
            && self.username_env.ends_with("_USERNAME")
            && !self.username_env.starts_with("CODETERM")
            && self
                .username_env
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
            && !capability
                .inherited_environment_exclusions
                .contains(&self.username_env)
            && bounded(&self.password_env, 128)
            && self.password_env.ends_with("_PASSWORD")
            && !self.password_env.starts_with("CODETERM")
            && self
                .password_env
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
            && !capability
                .inherited_environment_exclusions
                .contains(&self.password_env)
            && valid_path(&self.event_path, false)
            && capability.notification_methods.contains(&self.event_method)
            && self.response_headers.len() <= 16
            && self.response_headers.iter().all(|name| {
                bounded(name, 128)
                    && name.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                    })
                    && !matches!(
                        name.as_str(),
                        "set-cookie" | "authorization" | "proxy-authorization" | "www-authenticate"
                    )
            })
            && self.request_events.len() <= 32
            && self.request_events.iter().all(|(method, event)| {
                let pointer = |value: &str| bounded(value, 128) && value.starts_with('/');
                bounded(method, 128)
                    && !method.chars().any(char::is_whitespace)
                    && pointer(&event.type_pointer)
                    && pointer(&event.id_pointer)
                    && pointer(&event.thread_pointer)
                    && event.path_params.len() <= 8
                    && event
                        .path_params
                        .iter()
                        .all(|(name, value)| bounded(name, 128) && pointer(value))
                    && serde_json::to_string(&event.reject_body)
                        .is_ok_and(|body| body.len() <= 4096)
                    && capability
                        .request_methods
                        .get("answer")
                        .is_some_and(|methods| {
                            methods.contains(&event.reply_method)
                                && methods.contains(&event.reject_method)
                        })
            })
            && !self.routes.is_empty()
            && self.routes.len() <= 128
            && capability
                .command
                .args
                .iter()
                .any(|arg| arg.contains("{httpPort}"))
            && self.routes.iter().all(|(name, route)| {
                capability
                    .request_methods
                    .values()
                    .any(|methods| methods.contains(name))
                    && matches!(
                        route.method.as_str(),
                        "GET" | "POST" | "PUT" | "PATCH" | "DELETE"
                    )
                    && valid_path(&route.path, true)
            })
    }
}

pub fn valid_path(path: &str, templates: bool) -> bool {
    path.starts_with('/')
        && path.len() <= 1024
        && path.split('/').skip(1).all(|segment| {
            if templates && segment.starts_with('{') && segment.ends_with('}') {
                let name = &segment[1..segment.len() - 1];
                return !name.is_empty()
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
            }
            !matches!(segment, "." | "..")
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_-./".contains(&byte))
        })
}

fn is_false(value: &bool) -> bool {
    !value
}
