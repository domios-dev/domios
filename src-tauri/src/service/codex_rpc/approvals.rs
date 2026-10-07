use super::{
    lifecycle::for_tab,
    protocol::{RequestId, RpcError},
};
use crate::{
    multiplexer::types::{PendingPrompt, PromptOption},
    service::ServiceContext,
};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    sync::LazyLock,
};

#[derive(Clone)]
pub struct Request {
    pub jsonrpc_id: RequestId,
    pub kind: String,
    pub params: Value,
    question: usize,
    answers: serde_json::Map<String, Value>,
}

static REQUESTS: LazyLock<Mutex<HashMap<String, VecDeque<Request>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

impl Request {
    pub fn new(jsonrpc_id: RequestId, kind: String, params: Value) -> Self {
        Self {
            jsonrpc_id,
            kind,
            params,
            question: 0,
            answers: Default::default(),
        }
    }
    fn questions(&self) -> Option<&Vec<Value>> {
        self.params.get("questions")?.as_array()
    }
    pub fn prompt(&self) -> Option<PendingPrompt> {
        let (question, labels) = match self.kind.as_str() {
            "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => (
                self.params
                    .get("command")
                    .or_else(|| self.params.get("reason"))
                    .and_then(Value::as_str)
                    .unwrap_or("Approve this operation?")
                    .to_owned(),
                vec![
                    "Approve".into(),
                    "Approve for session".into(),
                    "Decline".into(),
                    "Cancel".into(),
                ],
            ),
            "item/permissions/requestApproval" => (
                format!(
                    "Grant permissions: {}",
                    self.params.get("permissions").unwrap_or(&Value::Null)
                ),
                vec![
                    "Grant for turn".into(),
                    "Grant for session".into(),
                    "Deny".into(),
                ],
            ),
            "item/tool/requestUserInput" => {
                let question = self.questions()?.get(self.question)?;
                let labels = question
                    .get("options")
                    .and_then(Value::as_array)
                    .map(|options| {
                        options
                            .iter()
                            .filter_map(|option| {
                                option
                                    .get("label")
                                    .and_then(Value::as_str)
                                    .map(str::to_owned)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                (
                    question
                        .get("question")
                        .and_then(Value::as_str)
                        .unwrap_or("Answer the question")
                        .into(),
                    labels,
                )
            }
            "mcpServer/elicitation/request" if renderable_elicitation(&self.params) => (
                self.params
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Respond to the tool request")
                    .into(),
                vec!["Accept".into(), "Decline".into(), "Cancel".into()],
            ),
            _ => return None,
        };
        let route = serde_json::to_string(&self.jsonrpc_id).ok()?;
        let mut options: Vec<_> = labels
            .into_iter()
            .enumerate()
            .map(|(i, label)| PromptOption {
                label,
                value: format!("{route}:{}:{i}", self.question),
            })
            .collect();
        if self.kind == "item/tool/requestUserInput" {
            options.push(PromptOption {
                label: "Other".into(),
                value: format!("{route}:{}:custom", self.question),
            });
        }
        Some(PendingPrompt {
            question,
            options,
            sections: None,
            multi_select: None,
            submit_available: None,
            selected_values: None,
        })
    }
    pub fn answer(&mut self, value: &str) -> Result<Option<Value>, String> {
        let selected = self.prompt().and_then(|prompt| {
            prompt
                .options
                .iter()
                .position(|option| option.value == value)
        });
        let index = selected.or_else(|| {
            value
                .parse::<usize>()
                .ok()
                .and_then(|index| index.checked_sub(1))
        });
        let result = match self.kind.as_str() {
            "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
                let decision = index
                    .and_then(|i| ["accept", "acceptForSession", "decline", "cancel"].get(i))
                    .ok_or("Invalid approval option")?;
                json!({"decision":decision})
            }
            "item/permissions/requestApproval" => match index {
                Some(0 | 1) => {
                    json!({"permissions":self.params.get("permissions").cloned().unwrap_or_else(|| json!({})),"scope":if index == Some(1) {"session"} else {"turn"}})
                }
                Some(2) => json!({"permissions":{},"scope":"turn"}),
                _ => return Err("Invalid permissions option".into()),
            },
            "item/tool/requestUserInput" => {
                let questions = self.questions().ok_or("Missing questions")?;
                let question = questions.get(self.question).ok_or("Missing question")?;
                let id = question
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or("Missing question id")?
                    .to_owned();
                let has_options = question
                    .get("options")
                    .and_then(Value::as_array)
                    .is_some_and(|options| !options.is_empty());
                let answer = match index.filter(|_| has_options) {
                    Some(i) => question
                        .get("options")
                        .and_then(Value::as_array)
                        .and_then(|options| options.get(i))
                        .and_then(|option| option.get("label"))
                        .and_then(Value::as_str)
                        .ok_or("Invalid question option")?
                        .to_owned(),
                    None if !value.trim().is_empty() => value.into(),
                    _ => return Err("Missing answer".into()),
                };
                let count = questions.len();
                self.answers.insert(id, json!({"answers":[answer]}));
                self.question += 1;
                if self.question < count {
                    return Ok(None);
                }
                json!({"answers":self.answers})
            }
            "mcpServer/elicitation/request" => match index {
                Some(0) if renderable_elicitation(&self.params) => {
                    json!({"action":"accept","content":{}})
                }
                Some(1) => json!({"action":"decline"}),
                Some(2) => json!({"action":"cancel"}),
                _ => return Err("Invalid elicitation option".into()),
            },
            _ => return Err("Unsupported server request".into()),
        };
        Ok(Some(result))
    }
}

fn renderable_elicitation(params: &Value) -> bool {
    params.get("requestedSchema").is_some_and(|schema| {
        schema.get("type").and_then(Value::as_str) == Some("object")
            && schema
                .get("properties")
                .and_then(Value::as_object)
                .is_some_and(|properties| properties.is_empty())
            && schema
                .get("required")
                .and_then(Value::as_array)
                .is_none_or(|required| required.is_empty())
    })
}

pub fn pending(tab: &str) -> Option<PendingPrompt> {
    REQUESTS.lock().get(tab)?.front()?.prompt()
}

pub fn receive(svc: &ServiceContext, tab: &str, id: RequestId, kind: String, params: Value) {
    let Some(sidecar) = for_tab(tab) else { return };
    let request = Request::new(id.clone(), kind.clone(), params);
    if request.prompt().is_none() {
        let result = if kind == "mcpServer/elicitation/request" {
            Ok(json!({"action":"decline"}))
        } else {
            Err(RpcError {
                code: -32601,
                message: "No registered handler for this server request".into(),
                data: None,
            })
        };
        if let Err(error) = sidecar.client.reply(&id, result) {
            log::warn!("[rpc-approval] reply failed: {error:?}");
        }
        return;
    }
    {
        let mut requests = REQUESTS.lock();
        let queue = requests.entry(tab.into()).or_default();
        if queue.iter().any(|request| request.jsonrpc_id == id) {
            return;
        }
        queue.push_back(request);
    }
    svc.push_structured_prompt(tab, pending(tab));
}

fn remove_request(tab: &str, id: &RequestId) {
    if let Some(queue) = REQUESTS.lock().get_mut(tab) {
        queue.retain(|request| &request.jsonrpc_id != id);
    }
}

pub fn resolved(svc: &ServiceContext, tab: &str, id: &RequestId) {
    remove_request(tab, id);
    svc.push_structured_prompt(tab, pending(tab));
}

pub fn clear_tab(tab: &str) {
    REQUESTS.lock().remove(tab);
}

pub(crate) fn respond(
    svc: &ServiceContext,
    tab: &str,
    value: &str,
) -> Option<Result<crate::service::structured_prompt::StructuredResponse, String>> {
    let sidecar = for_tab(tab)?;
    let outcome = (|| {
        let (advanced, option_index) = {
            let mut requests = REQUESTS.lock();
            let queue = requests.get_mut(tab).ok_or("No pending RPC request")?;
            let request = queue.front_mut().ok_or("No pending RPC request")?;
            let option_index = request
                .prompt()
                .and_then(|prompt| {
                    prompt
                        .options
                        .iter()
                        .position(|option| option.value == value)
                })
                .unwrap_or_else(|| value.parse::<usize>().unwrap_or(1).saturating_sub(1));
            let mut next = request.clone();
            match next.answer(value)? {
                Some(result) => {
                    sidecar
                        .client
                        .reply(&request.jsonrpc_id, Ok(result))
                        .map_err(super::lifecycle::rpc_error)?;
                    queue.pop_front();
                    (false, option_index)
                }
                None => {
                    *request = next;
                    (true, option_index)
                }
            }
        };
        svc.push_structured_prompt(tab, pending(tab));
        if advanced {
            return Ok(crate::service::structured_prompt::StructuredResponse::Advanced);
        }
        Ok(crate::service::structured_prompt::StructuredResponse::Answered { option_index })
    })();
    Some(outcome)
}

#[cfg(test)]
mod tests;
